//! Aesthetic, readable PR comment markdown (CodeRabbit / PR-Agent inspired).

use crate::bot_runtime::BotRuntimeConfig;
use crate::config::Config;
use crate::detectors::{Finding, Findings};
use std::fmt::Write;

/// Severity badge without emoji noise.
pub fn severity_badge(severity: &str) -> &'static str {
    match severity {
        "blocking" => "`blocking`",
        "warning" => "`warning`",
        _ => "`info`",
    }
}

/// Short fingerprint for display (first 12 hex chars).
pub fn short_fp(finding: &Finding) -> String {
    let fp = finding.fingerprint();
    fp.chars().take(12).collect()
}

/// Inline finding comment — scannable, actionable, dismissable.
pub fn inline_finding_comment(f: &Finding) -> String {
    let badge = severity_badge(f.severity);
    let title = finding_title(f);
    let why = finding_why(f);
    let action = finding_action(f);
    let fp = short_fp(f);

    let mut body = format!(
        "**{title}** · {badge}\n\n{why}\n\n**Fix:** {action}\n\n---\n`fingerprint: {fp}` · dismiss with `@codasaurus ignore {fp}`"
    );

    if let Some(ref code) = f.codemod {
        let _ = write!(
            body,
            "\n\n```suggestion\n{}\n```",
            code.trim_end()
        );
    }

    body
}

fn finding_title(f: &Finding) -> String {
    match f.detector.as_str() {
        "hallucinated-imports" => format!("Package does not exist — `{}`", pkg(f)),
        "secrets" => "Credential in source".into(),
        "phantom-deps" => format!("Undeclared dependency — `{}`", pkg(f)),
        "todo-leaks" => "Incomplete code marker".into(),
        "vulnerabilities" => format!("Known vulnerability — `{}`", pkg_from_suggestion(f)),
        "over-engineering" => "Unnecessary abstraction".into(),
        "boilerplate" => "Boilerplate code".into(),
        "stale-api" => "Deprecated API".into(),
        "graph" => "Unused code".into(),
        "guidelines" => "Guideline violation".into(),
        "slop" => "AI-generated PR signals".into(),
        other => other.replace('-', " "),
    }
}

fn finding_why(f: &Finding) -> String {
    match f.detector.as_str() {
        "hallucinated-imports" => format!(
            "`{}` was not found on the package registry. This import will fail in CI or at runtime.",
            pkg(f)
        ),
        "secrets" => {
            "A secret appears hardcoded in this change. Rotate the credential and move it to a secret store or environment variable.".into()
        }
        "phantom-deps" => format!(
            "`{}` is imported but missing from the project manifest. Fresh installs and CI will break.",
            pkg(f)
        ),
        "todo-leaks" => {
            "A `TODO` / `FIXME` marker was committed. Finish the work or remove the marker before merge.".into()
        }
        "vulnerabilities" => f.message.clone(),
        other => {
            if f.message.trim().is_empty() {
                format!("Detected by `{other}`.")
            } else {
                // Redact obvious secret-looking spans in messages
                redact_secrets(&f.message)
            }
        }
    }
}

fn finding_action(f: &Finding) -> String {
    f.suggestion
        .clone()
        .filter(|s| !s.trim().is_empty())
        .map(|s| redact_secrets(&s))
        .unwrap_or_else(|| match f.detector.as_str() {
            "hallucinated-imports" => "Replace with a real package or remove the import.".into(),
            "secrets" => "Remove the secret, rotate it, and load from the environment.".into(),
            "phantom-deps" => "Add the package to the project manifest.".into(),
            "todo-leaks" => "Complete the implementation or delete the marker.".into(),
            _ => "Address this finding before merge.".into(),
        })
}

fn pkg(f: &Finding) -> String {
    f.message
        .split('`')
        .nth(1)
        .unwrap_or("package")
        .to_string()
}

fn pkg_from_suggestion(f: &Finding) -> String {
    f.suggestion
        .as_ref()
        .and_then(|s| s.split('`').nth(1))
        .unwrap_or("package")
        .to_string()
}

/// Redact long high-entropy tokens that look like secrets.
pub fn redact_secrets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for token in s.split_whitespace() {
        if token.len() >= 24
            && token
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .count()
                >= 20
        {
            let keep = token.chars().take(4).collect::<String>();
            out.push_str(&format!("{keep}…[redacted]"));
        } else {
            out.push_str(token);
        }
        out.push(' ');
    }
    out.trim_end().to_string()
}

/// Build the main walkthrough / summary comment body.
#[allow(clippy::too_many_arguments, dead_code)]
pub fn walkthrough_body(
    findings: &Findings,
    has_blocking: bool,
    pr_title: &str,
    files: &[serde_json::Value],
    reviewers: &[String],
    config: &Config,
    runtime: &BotRuntimeConfig,
    include_brand_gif: bool,
    include_mermaid: bool,
) -> String {
    walkthrough_body_ext(
        findings,
        has_blocking,
        pr_title,
        files,
        reviewers,
        config,
        runtime,
        include_brand_gif,
        include_mermaid,
        &[],
        "",
    )
}

/// Extended walkthrough with related PRs + linked-issue assessment markdown.
#[allow(clippy::too_many_arguments)]
pub fn walkthrough_body_ext(
    findings: &Findings,
    has_blocking: bool,
    pr_title: &str,
    files: &[serde_json::Value],
    reviewers: &[String],
    config: &Config,
    runtime: &BotRuntimeConfig,
    include_brand_gif: bool,
    include_mermaid: bool,
    related_prs: &[String],
    issue_assessment_md: &str,
) -> String {
    let counts = findings.count_by_severity();
    let blocking = *counts.get("blocking").unwrap_or(&0);
    let warning = *counts.get("warning").unwrap_or(&0);
    let info = *counts.get("info").unwrap_or(&0);
    let total = findings.findings.len();

    let verdict = if findings.is_empty() {
        "**Verdict:** ship"
    } else if has_blocking {
        "**Verdict:** hold — blocking findings"
    } else {
        "**Verdict:** fix-before-ship"
    };

    let mut body = String::new();

    if include_brand_gif && runtime.max_inline_comments > 0 {
        let _ = writeln!(body, "<sub>Codasaurus review</sub>\n");
    }

    let _ = writeln!(body, "### Codasaurus review\n");
    let _ = writeln!(body, "{verdict}\n");
    let _ = writeln!(
        body,
        "| Severity | Count |\n| --- | ---: |\n| blocking | {blocking} |\n| warning | {warning} |\n| info | {info} |\n| **total** | **{total}** |\n"
    );

    let _ = writeln!(body, "<details>");
    let _ = writeln!(body, "<summary><strong>Walkthrough</strong></summary>\n");
    let _ = writeln!(body, "PR: _{}_\n", escape_md(pr_title));

    if include_mermaid {
        if let Some(diagram) = mermaid_change_flow(files) {
            let _ = writeln!(body, "```mermaid\n{diagram}\n```\n");
        }
    }

    if !issue_assessment_md.is_empty() {
        body.push_str(issue_assessment_md);
    }

    if !related_prs.is_empty() {
        let _ = writeln!(body, "#### Related PRs\n");
        for r in related_prs.iter().take(8) {
            let _ = writeln!(body, "- {r}");
        }
        let _ = writeln!(body);
    }

    let _ = writeln!(body, "#### Changed files\n");
    let _ = writeln!(body, "| File | Status |");
    let _ = writeln!(body, "| --- | --- |");
    for file in files.iter().take(40) {
        let name = file["filename"].as_str().unwrap_or("?");
        let status = file["status"].as_str().unwrap_or("modified");
        let _ = writeln!(body, "| `{name}` | {status} |");
    }
    if files.len() > 40 {
        let _ = writeln!(body, "| … | {} more |", files.len() - 40);
    }
    let _ = writeln!(body);

    if !findings.is_empty() {
        let _ = writeln!(body, "#### Findings\n");
        for f in findings.findings.iter().take(30) {
            let fp = short_fp(f);
            let _ = writeln!(
                body,
                "- {} {} — `{}:{}` — `{fp}`",
                severity_badge(f.severity),
                finding_title(f),
                f.file,
                f.line
            );
        }
        if findings.findings.len() > 30 {
            let _ = writeln!(
                body,
                "\n_{} more findings omitted from walkthrough._",
                findings.findings.len() - 30
            );
        }
        let _ = writeln!(body);
    }

    let _ = writeln!(body, "#### Pre-merge checks\n");
    let checks = [
        (!has_blocking, "No blocking findings"),
        (
            warning <= config.pre_merge.max_warnings,
            "Warning budget within limit",
        ),
        (!pr_title.trim().is_empty(), "PR has a title"),
    ];
    for (ok, label) in checks {
        let box_ = if ok { "[x]" } else { "[ ]" };
        let _ = writeln!(body, "- {box_} {label}");
    }
    let _ = writeln!(body);

    if !reviewers.is_empty() {
        let _ = writeln!(
            body,
            "**Suggested reviewers:** {}",
            reviewers
                .iter()
                .map(|r| format!("@{r}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(body);
    }

    let _ = writeln!(body, "</details>\n");

    let _ = writeln!(
        body,
        "<details><summary>Commands</summary>\n\n\
         - `@codasaurus review` — re-run review\n\
         - `@codasaurus describe` — PR summary / walkthrough\n\
         - `@codasaurus summarize` — short executive summary\n\
         - `@codasaurus improve` — actionable suggestions\n\
         - `@codasaurus security` — secrets / vuln-focused scan\n\
         - `@codasaurus labels` — suggest and apply labels\n\
         - `@codasaurus changelog` / `update_changelog` — Keep a Changelog draft\n\
         - `@codasaurus add_docs` — docs stubs\n\
         - `@codasaurus similar` — related PRs by path history\n\
         - `@codasaurus fix` — apply available codemods (opt-in)\n\
         - `@codasaurus ask <question>` — ask about this PR\n\
         - `@codasaurus ignore <fingerprint>` — dismiss a finding\n\
         - `@codasaurus help` — show commands\n\n\
         </details>"
    );

    if body.len() > runtime.max_comment_bytes {
        truncate_utf8_owned(&mut body, runtime.max_comment_bytes);
    }
    body
}

pub fn clean_approve_body() -> String {
    "### Codasaurus review\n\n**Verdict:** ship\n\nNo issues found.".into()
}

pub fn help_body() -> String {
    "### Codasaurus commands\n\n\
     | Command | What it does |\n\
     | --- | --- |\n\
     | `@codasaurus review` | Run static (+ optional LLM) review |\n\
     | `@codasaurus describe` | Generate PR walkthrough / summary |\n\
     | `@codasaurus summarize` | Short PR summary |\n\
     | `@codasaurus improve` | Post actionable improvement suggestions |\n\
     | `@codasaurus security` | Secrets / vuln-focused scan |\n\
     | `@codasaurus labels` | Suggest and apply PR labels |\n\
     | `@codasaurus changelog` / `update_changelog` | Draft a Keep a Changelog section |\n\
     | `@codasaurus add_docs` | Suggest README/docs stubs for this PR |\n\
     | `@codasaurus similar` | Related PRs by path history |\n\
     | `@codasaurus fix` | Apply available codemods (opt-in) |\n\
     | `@codasaurus ask …` | Answer a question about this PR |\n\
     | `@codasaurus ignore <fp>` | Dismiss a finding by fingerprint |\n\
     | `@codasaurus help` | Show this help |\n"
        .into()
}

fn escape_md(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Compact mermaid flowchart of top path prefixes (shown when LLM is on).
fn mermaid_change_flow(files: &[serde_json::Value]) -> Option<String> {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for f in files {
        let name = f["filename"].as_str().unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let top = name
            .split('/')
            .next()
            .unwrap_or(name)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .take(24)
            .collect::<String>();
        if top.is_empty() {
            continue;
        }
        *counts.entry(top).or_default() += 1;
    }
    if counts.is_empty() {
        return None;
    }
    let mut nodes: Vec<(String, usize)> = counts.into_iter().collect();
    nodes.sort_by(|a, b| b.1.cmp(&a.1));
    nodes.truncate(6);

    let mut out = String::from("flowchart LR\n  Author --> Review[Codasaurus]\n");
    for (i, (name, n)) in nodes.iter().enumerate() {
        let id = format!("N{i}");
        let _ = writeln!(out, "  Review --> {id}[{name} ×{n}]");
    }
    Some(out)
}

fn truncate_utf8_owned(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }
    let mut idx = max_bytes.saturating_sub(20);
    while !s.is_char_boundary(idx) {
        idx -= 1;
    }
    s.truncate(idx);
    s.push_str("\n\n_…truncated_");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_long_tokens() {
        let s = redact_secrets("key=abcdefghijklmnopqrstuvwxyz0123456789");
        assert!(s.contains("redacted"));
    }

    #[test]
    fn short_fp_len() {
        let f = Finding {
            detector: "secrets".into(),
            severity: "blocking",
            file: "a.rs".into(),
            line: 1,
            column: 0,
            message: "x".into(),
            suggestion: None,
            evidence: None,
            codemod: None,
        };
        assert_eq!(short_fp(&f).len(), 12);
    }
}
