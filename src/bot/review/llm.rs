use crate::detectors::{Finding, Findings};
use crate::state::ReviewState;
use anyhow::Result;
use std::fmt::Write;

use super::github::post_or_update_comment;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn maybe_post_auto_improve(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    pr_number: i64,
    files: &[serde_json::Value],
    llm_cfg: &crate::llm::LlmConfig,
    review_ctx: &crate::llm::ReviewContext,
    state: &Option<ReviewState>,
    max_diff_chars: usize,
    max_issues: usize,
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

    let output = crate::llm::review_diff(&diff, llm_cfg, Some(review_ctx)).await?;
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
