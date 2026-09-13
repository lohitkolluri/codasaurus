use crate::detectors::{Finding, Findings};
use crate::state::ReviewState;
use anyhow::Result;
use std::fmt::Write;

use super::github::{post_or_update_comment, review_exists_with_marker};

/// Marks the body of the LLM auto-fix suggestion review so its own idempotency
/// check never collides with the Tier-1 review already posted for the same commit.
const AUTO_FIX_MARKER: &str = "<!-- codasaurus:auto-fix -->";

#[allow(clippy::too_many_arguments)]
pub(crate) async fn maybe_post_auto_improve(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    pr_number: i64,
    head_sha: &str,
    files: &[serde_json::Value],
    llm_cfg: &crate::llm::LlmConfig,
    review_ctx: &crate::llm::ReviewContext,
    state: &Option<ReviewState>,
    max_diff_chars: usize,
    max_issues: usize,
    mcp_tools: &[crate::mcp::McpToolSpec],
    pool: Option<&crate::db::DbPool>,
    semantic_index_enabled: bool,
) -> Result<()> {
    let llm_files = crate::llm::filter_llm_files(files);
    if llm_files.is_empty() {
        tracing::info!("skipping auto review_diff: no high-signal patches after path filter");
        return Ok(());
    }

    let mut diff = String::new();
    for f in llm_files.iter().take(40) {
        let name = f["filename"].as_str().unwrap_or("?");
        let patch = f["patch"].as_str().unwrap_or("");
        if patch.is_empty() {
            continue;
        }
        let _ = write!(diff, "--- a/{name}\n+++ b/{name}\n{patch}\n");
        if diff.len() > max_diff_chars {
            break;
        }
    }
    if diff.is_empty() {
        return Ok(());
    }

    let mut grounded_ctx = review_ctx.clone();
    let patches: Vec<(String, String)> = llm_files
        .iter()
        .filter_map(|f| {
            let name = f["filename"].as_str()?.to_string();
            let patch = f["patch"].as_str().unwrap_or("").to_string();
            if patch.is_empty() {
                None
            } else {
                Some((name, patch))
            }
        })
        .collect();
    let paths: Vec<String> = patches.iter().map(|(p, _)| p.clone()).collect();
    let mut grounding = crate::bot::grounding::build_grounding_block(&paths, &patches);
    if semantic_index_enabled {
        if let Some(pool) = pool {
            let changed_symbols: Vec<String> = patches
                .iter()
                .flat_map(|(_, patch)| crate::bot::grounding::extract_symbols(patch).into_iter())
                .collect();
            if !changed_symbols.is_empty() {
                let semantic_block = crate::index::semantic::related_symbols(
                    pool,
                    llm_cfg,
                    repo_name,
                    &changed_symbols,
                    &paths,
                    8,
                )
                .await
                .unwrap_or_default();
                if !semantic_block.is_empty() {
                    let mut block = String::from(
                        "## Cross-repo context (semantically related, not directly imported)\n",
                    );
                    for s in &semantic_block {
                        let _ =
                            writeln!(block, "- `{}:{}` `{}`", s.file_path, s.line, s.symbol_name);
                    }
                    if grounding.is_empty() {
                        grounding = block;
                    } else {
                        grounding.push('\n');
                        grounding.push_str(&block);
                    }
                }
            }
        }
    }
    if !grounding.is_empty() {
        grounded_ctx.repo_context = Some(match grounded_ctx.repo_context.take() {
            Some(existing) if !existing.trim().is_empty() => format!("{existing}\n\n{grounding}"),
            _ => grounding,
        });
    }

    let output = crate::llm::review_diff(&diff, llm_cfg, Some(&grounded_ctx), mcp_tools).await?;
    let known_paths: Vec<String> = files
        .iter()
        .filter_map(|f| f["filename"].as_str().map(str::to_string))
        .collect();
    let file_contents: Vec<(String, String)> = llm_files
        .iter()
        .filter_map(|f| {
            let name = f["filename"].as_str()?.to_string();
            let patch = f["patch"].as_str().unwrap_or("").to_string();
            if patch.is_empty() {
                None
            } else {
                Some((name, patch))
            }
        })
        .collect();
    let issues =
        crate::bot::provenance::reverify_llm_issues(&output.issues, &known_paths, &file_contents);
    if issues.is_empty() {
        return Ok(());
    }

    let mut text = String::from("### Codasaurus improve (auto)\n\n");
    if let Some(summary) = output.summary.as_deref().filter(|s| !s.is_empty()) {
        let _ = writeln!(text, "{summary}\n");
    }
    text.push_str("| File | Line | Severity | Conf | Suggestion | Source |\n| --- | ---: | --- | --- | --- | --- |\n");
    for issue in issues.iter().take(max_issues.max(1)) {
        let sug = issue
            .suggestion
            .as_deref()
            .unwrap_or(&issue.description)
            .replace('|', "\\|")
            .chars()
            .take(140)
            .collect::<String>();
        let _ = writeln!(
            text,
            "| `{}` | {} | `{}` | `{}` | {sug} | `llm` |",
            issue.file, issue.line, issue.severity, issue.confidence
        );
    }
    text.push_str(
        "\n<details>\n<summary>Notes</summary>\n\n\
         LLM findings were re-verified (path + confidence + evidence) before posting. \
         Low-confidence issues are dropped automatically.\n\
         Enable with repo `config_json.auto_review_diff: true` (opt-in; skipped when Tier-1 blocks).\n\n\
         </details>\n",
    );

    post_or_update_comment(
        client,
        auth_header,
        repo_name,
        pr_number,
        &text,
        state,
        "auto_improve",
    )
    .await?;

    post_auto_fix_suggestions(client, auth_header, repo_name, pr_number, head_sha, &issues).await?;

    Ok(())
}

/// Post verified LLM `replacement`s as a GitHub suggestion-fence review, reusing the
/// same rendering the Tier-1 pipeline uses for `Finding.codemod` (`inline_finding_comment`).
/// This is a second, independent review from Tier-1's — see `AUTO_FIX_MARKER` for why its
/// idempotency check can't reuse `review_exists_for_commit`.
async fn post_auto_fix_suggestions(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    pr_number: i64,
    head_sha: &str,
    issues: &[crate::llm::LlmIssue],
) -> Result<()> {
    let comments: Vec<serde_json::Value> = issues
        .iter()
        .filter_map(|issue| {
            let repl = issue.replacement.as_ref()?;
            let finding = Finding {
                detector: format!("llm-{}", issue.category),
                severity: "info",
                file: issue.file.clone(),
                line: issue.line,
                column: 0,
                message: issue.description.clone(),
                suggestion: issue.suggestion.clone(),
                evidence: None,
                codemod: Some(repl.replacement.clone()),
                confidence: None,
                judge_rationale: None,
                reachability: None,
            };
            Some(serde_json::json!({
                "path": finding.file,
                "line": finding.line,
                "side": "RIGHT",
                "body": crate::bot::markdown::inline_finding_comment(&finding),
            }))
        })
        .collect();
    if comments.is_empty() || head_sha.is_empty() {
        return Ok(());
    }

    if review_exists_with_marker(
        client,
        auth_header,
        repo_name,
        pr_number,
        head_sha,
        AUTO_FIX_MARKER,
    )
    .await
    .unwrap_or(false)
    {
        return Ok(());
    }

    let review_body = serde_json::json!({
        "body": format!(
            "{AUTO_FIX_MARKER}\n### Codasaurus auto-fix\n\n{} one-click suggestion{} from re-verified LLM findings.",
            comments.len(),
            if comments.len() == 1 { "" } else { "s" }
        ),
        "event": "COMMENT",
        "comments": comments,
    });
    let review_url = format!("https://api.github.com/repos/{repo_name}/pulls/{pr_number}/reviews");
    let resp = client
        .post(&review_url)
        .header("Authorization", auth_header)
        .header("Accept", "application/vnd.github+json")
        .header(
            "User-Agent",
            concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
        )
        .json(&review_body)
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(status = %status, body = %body.chars().take(400).collect::<String>(), "auto-fix suggestion review POST failed");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn generate_and_post_summary(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    pr_number: i64,
    findings: &Findings,
    llm_cfg: &crate::llm::LlmConfig,
    pr_title: &str,
    pr_body: &str,
    state: &Option<ReviewState>,
    review_ctx: &crate::llm::ReviewContext,
) -> Result<()> {
    let mut findings_text = String::new();
    if let Some(ref ctx) = review_ctx.repo_context {
        let _ = writeln!(findings_text, "Repo context:\n{ctx}\n");
    }
    if !review_ctx.linked_issues.is_empty() {
        let _ = writeln!(findings_text, "Linked issues:");
        for iss in &review_ctx.linked_issues {
            let _ = writeln!(findings_text, "- #{} {}", iss.number, iss.title);
        }
        findings_text.push('\n');
    }
    // Prefer blocking findings first; cap volume for token cost.
    let mut ordered: Vec<&Finding> = findings.findings.iter().collect();
    ordered.sort_by_key(|f| match f.severity {
        "blocking" => 0,
        "warning" => 1,
        _ => 2,
    });
    for f in ordered.iter().take(40) {
        let _ = writeln!(
            findings_text,
            "- {}: {} (line {})",
            f.severity, f.message, f.line
        );
    }

    let summary = crate::llm::summarize_pr(pr_title, pr_body, &findings_text, llm_cfg).await?;

    let summary_body = format!(
        "### Codasaurus summary\n\n{summary}\n\n---\n<sub>LLM summary · Tier-1 findings remain authoritative</sub>"
    );

    post_or_update_comment(
        client,
        auth_header,
        repo_name,
        pr_number,
        &summary_body,
        state,
        "llm_summary",
    )
    .await?;

    Ok(())
}
