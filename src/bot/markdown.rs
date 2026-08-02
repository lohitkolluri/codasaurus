//! Aesthetic, readable PR comment markdown for GitHub review threads.
//!
//! Uses shields.io badge images (GitHub renders them inline) plus compact tables
//! and `<details>` sections for a scannable walkthrough.

use crate::bot_runtime::BotRuntimeConfig;
use crate::config::Config;
use crate::detectors::{Finding, Findings};
use std::fmt::Write;

/// shields.io badge (flat-square) — safe for GitHub PR markdown.
fn shield(label: &str, message: &str, color: &str) -> String {
    let label = urlencoding_lite(label);
    let message = urlencoding_lite(message);
    format!(
        "<img src=\"https://img.shields.io/badge/{label}-{message}-{color}?style=flat-square\" alt=\"{label}: {message}\" />"
    )
}

/// Minimal path-segment encoder for shields.io (`-` → `--`, `_` → `__`, space → `_`).
fn urlencoding_lite(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '-' => "--".to_string(),
            '_' => "__".to_string(),
            ' ' => "_".to_string(),
            '/' => "%2F".to_string(),
            '?' => "%3F".to_string(),
            '#' => "%23".to_string(),
            '&' => "%26".to_string(),
            c if c.is_ascii_alphanumeric() => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

/// Severity badge — shields for the summary table; monospace in tight lists.
pub fn severity_badge(severity: &str) -> String {
    match severity {
        "blocking" => shield("sev", "blocking", "e11d48"),
        "warning" => shield("sev", "warning", "f59e0b"),
        _ => shield("sev", "info", "64748b"),
    }
}

pub fn ask_header() -> String {
    format!(
        "### {} Codasaurus ask\n",
        shield("codasaurus", "ask", "0ea5e9")
    )
}

/// Short fingerprint for display (first 12 hex chars).
pub fn short_fp(finding: &Finding) -> String {
    let fp = finding.fingerprint();
    fp.chars().take(12).collect()
}

/// Shared compact commands footer for review / describe comments.
pub fn commands_details() -> String {
    "<details>\n<summary><strong>Commands</strong> · type as plain text (GitHub Apps are not @-mentionable)</summary>\n\n\
     | Command | Action |\n\
     | --- | --- |\n\
     | `@codasaurus review` | Re-run review |\n\
     | `@codasaurus describe` | Walkthrough / summary |\n\
     | `@codasaurus summarize` | Executive summary |\n\
     | `@codasaurus improve` | Actionable suggestions |\n\
     | `@codasaurus security` | Secrets / vuln scan |\n\
     | `@codasaurus labels` | Suggest labels |\n\
     | `@codasaurus changelog` | Keep a Changelog draft |\n\
     | `@codasaurus add_docs` | Docs stubs |\n\
     | `@codasaurus similar` | Related PRs |\n\
     | `@codasaurus impact` | Blast-radius estimate |\n\
     | `@codasaurus fix` / `fix <fp>` | Apply codemods (opt-in) |\n\
     | `@codasaurus digest` | Weekly rollup |\n\
     | `@codasaurus ask …` | Ask about this PR |\n\
     | `@codasaurus ignore <fp>` | Dismiss a finding |\n\
     | `@codasaurus help` | Full command list |\n\n\
     <sub>Tip: type <code>@codasaurus ask …</code> or <code>codasaurus ask …</code> — no autocomplete user needed.</sub>\n\n\
     </details>\n"
        .into()
}

/// Inline finding comment — scannable, actionable, dismissable.
pub fn inline_finding_comment(f: &Finding) -> String {
    let badge = severity_badge(f.severity);
    let title = finding_title(f);
    let why = finding_why(f);
    let action = finding_action(f);
    let fp = short_fp(f);
    let provenance = crate::bot::provenance::provenance_line(f);

    let fix_cta = if f.codemod.as_ref().is_some_and(|c| !c.is_empty()) {
        format!(" · `@codasaurus fix {fp}`")
    } else {
        String::new()
    };
    let mut body = format!(
        "{badge} **{title}**\n\n\
         {why}\n\n\
         **Fix:** {action}\n\n\
         <details>\n<summary>Provenance</summary>\n\n{provenance}\n\n</details>\n\n\
         ---\n\
         <sub><code>fingerprint: {fp}</code> · <code>@codasaurus ignore {fp}</code>{fix_cta} · 👎 to dismiss</sub>"
    );

    if let Some(ref code) = f.codemod {
        let _ = write!(
            body,
            "\n\n```suggestion\n{}\n```\n\n<sub>Or reply `@codasaurus fix {fp}` (needs Contents Write + allow_auto_fix).</sub>",
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
        "iac" => "Infrastructure risk".into(),
        "policy" => "Policy pack violation".into(),
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
        "vulnerabilities" => redact_secrets(&f.message),
        other => {
            if f.message.trim().is_empty() {
                format!("Detected by `{other}`.")
            } else {
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
            "vulnerabilities" => "Upgrade to a patched version or remove the dependency.".into(),
            "iac" => "Tighten the privilege / network exposure before merge.".into(),
            _ => "Address this finding before merge.".into(),
        })
}

fn pkg(f: &Finding) -> String {
    f.message.split('`').nth(1).unwrap_or("package").to_string()
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
        if token.len() >= 24 && token.chars().filter(|c| c.is_ascii_alphanumeric()).count() >= 20 {
            let keep = token.chars().take(4).collect::<String>();
            out.push_str(&format!("{keep}…[redacted]"));
        } else {
            out.push_str(token);
        }
        out.push(' ');
    }
    out.trim_end().to_string()
}

/// Optional walkthrough sections (blast radius, dep delta, agent badge, …).
#[derive(Debug, Clone, Default)]
pub struct WalkthroughExtras<'a> {
    pub related_prs: &'a [String],
    pub issue_assessment_md: &'a str,
    pub agent_badge: Option<&'a str>,
    pub blast_md: &'a str,
    pub dep_delta_md: &'a str,
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
        WalkthroughExtras::default(),
    )
}

/// Extended walkthrough with related PRs + novelty sections.
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
    extras: WalkthroughExtras<'_>,
) -> String {
    let related_prs = extras.related_prs;
    let issue_assessment_md = extras.issue_assessment_md;
    let counts = findings.count_by_severity();
    let blocking = *counts.get("blocking").unwrap_or(&0);
    let warning = *counts.get("warning").unwrap_or(&0);
    let info = *counts.get("info").unwrap_or(&0);
    let total = findings.findings.len();

    let (verdict_label, verdict_color, verdict_detail) = if findings.is_empty() {
        ("ship", "22c55e", "No findings — ready to merge.")
    } else if has_blocking {
        (
            "hold",
            "e11d48",
            "Blocking findings must be fixed or dismissed.",
        )
    } else {
        (
            "review",
            "f59e0b",
            "Non-blocking findings remain — review before merge.",
        )
    };

    let mut body = String::with_capacity(3072);

    let brand = shield("codasaurus", "review", "111827");
    let verdict_badge = shield("verdict", verdict_label, verdict_color);
    let block_badge = shield(
        "blocking",
        &blocking.to_string(),
        if blocking > 0 { "e11d48" } else { "22c55e" },
    );
    let warn_badge = shield(
        "warning",
        &warning.to_string(),
        if warning > 0 { "f59e0b" } else { "94a3b8" },
    );
    let info_badge = shield("info", &info.to_string(), "64748b");
    let total_badge = shield("findings", &total.to_string(), "0ea5e9");

    let _ = writeln!(body, "### {brand} Codasaurus Review\n");
    let _ = writeln!(
        body,
        "{verdict_badge} {block_badge} {warn_badge} {info_badge} {total_badge}\n"
    );
    if include_brand_gif && runtime.max_inline_comments > 0 {
        let _ = writeln!(
            body,
            "<sub>Self-hosted · BYOK · fail-closed offline mode</sub>\n"
        );
    }
    if let Some(badge) = extras.agent_badge {
        let _ = writeln!(body, "{badge}\n");
    }
    let _ = writeln!(body, "> **{verdict_detail}**\n");

    // Walkthrough order: effort → diagram → files → issues → findings.
    let _ = writeln!(body, "<details open>");
    let _ = writeln!(
        body,
        "<summary><strong>Walkthrough</strong> — {}</summary>\n",
        escape_md(pr_title)
    );

    let effort = estimate_review_effort(files.len(), total, blocking);
    let effort_badge = shield("effort", &format!("{effort}/5"), effort_color(effort));
    let _ = writeln!(body, "#### Estimated review effort\n");
    let _ = writeln!(
        body,
        "{effort_badge} · **{}** file{} · **{total}** finding{}\n",
        files.len(),
        if files.len() == 1 { "" } else { "s" },
        if total == 1 { "" } else { "s" },
    );

    if include_mermaid {
        if let Some(diagram) = mermaid_change_flow(files) {
            let _ = writeln!(body, "#### Change map\n");
            let _ = writeln!(body, "```mermaid\n{diagram}\n```\n");
        }
    }

    if !issue_assessment_md.is_empty() {
        body.push_str(issue_assessment_md);
        if !issue_assessment_md.ends_with('\n') {
            body.push('\n');
        }
    }

    if !extras.blast_md.is_empty() {
        body.push_str(extras.blast_md);
    }

    if !extras.dep_delta_md.is_empty() {
        body.push_str(extras.dep_delta_md);
    }

    if !related_prs.is_empty() {
        let _ = writeln!(body, "#### Related PRs\n");
        for r in related_prs.iter().take(8) {
            let _ = writeln!(body, "- {r}");
        }
        let _ = writeln!(body);
    }

    write_changed_files_summary(&mut body, files);

    if !findings.is_empty() {
        let _ = writeln!(body, "#### Findings\n");
        for f in findings.findings.iter().take(30) {
            let fp = short_fp(f);
            let loc = if f.line > 0 {
                format!("`{}:{}`", f.file, f.line)
            } else {
                format!("`{}`", f.file)
            };
            let _ = writeln!(
                body,
                "- {} **{}** — {loc} · `{fp}`",
                severity_badge(f.severity),
                finding_title(f)
            );
        }
        if findings.findings.len() > 30 {
            let _ = writeln!(
                body,
                "\n_{} more findings omitted — see inline comments._",
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
        let _ = writeln!(body, "#### Suggested reviewers\n");
        let _ = writeln!(
            body,
            "{}",
            reviewers
                .iter()
                .map(|r| format!("@{r}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(body);
    }

    let _ = writeln!(body, "</details>\n");
    body.push_str(&commands_details());

    if body.len() > runtime.max_comment_bytes {
        truncate_utf8_owned(&mut body, runtime.max_comment_bytes);
    }
    body
}

/// Clean APPROVE body with optional novelty sections (agent / blast / dep-delta).
pub fn clean_approve_body_ext(
    agent_badge: Option<&str>,
    blast_md: &str,
    dep_delta_md: &str,
) -> String {
    let mut body = String::new();
    let brand = shield("codasaurus", "review", "111827");
    let verdict = shield("verdict", "ship", "22c55e");
    let findings = shield("findings", "0", "22c55e");
    let _ = writeln!(body, "### {brand} Codasaurus Review\n");
    let _ = writeln!(body, "{verdict} {findings}\n");
    let _ = writeln!(
        body,
        "> **No findings — ready to merge.** Tier-1 detectors found nothing to block.\n"
    );
    if let Some(badge) = agent_badge.filter(|s| !s.is_empty()) {
        body.push_str(badge);
        body.push('\n');
    }
    if !blast_md.is_empty() {
        body.push_str(blast_md);
        if !blast_md.ends_with('\n') {
            body.push('\n');
        }
    }
    if !dep_delta_md.is_empty() {
        body.push_str(dep_delta_md);
        if !dep_delta_md.ends_with('\n') {
            body.push('\n');
        }
    }
    body.push_str(&commands_details());
    body
}

pub fn help_body() -> String {
    let brand = shield("codasaurus", "commands", "8b5cf6");
    format!(
        "### {brand} Codasaurus commands\n\n\
         GitHub Apps **cannot be @-mentioned** like a user. Type the text literally:\n\n\
         ```text\n\
         @codasaurus ask why is this flaky?\n\
         codasaurus review\n\
         ```\n\n\
         | Command | What it does |\n\
         | --- | --- |\n\
         | `review` | Static (+ optional LLM) review |\n\
         | `describe` | Walkthrough / PR summary |\n\
         | `summarize` | Short executive summary |\n\
         | `improve` | Actionable improvement suggestions |\n\
         | `security` | Secrets / vuln-focused scan |\n\
         | `labels` | Suggest and apply PR labels |\n\
         | `changelog` / `update_changelog` | Keep a Changelog draft |\n\
         | `add_docs` | README / docs stubs |\n\
         | `similar` | Related PRs by path history |\n\
         | `impact` | Blast-radius estimate |\n\
         | `fix` / `fix <fp>` | Apply available codemods (opt-in) |\n\
         | `digest` | Weekly review rollup |\n\
         | `ask …` | Answer a question about this PR |\n\
         | `ignore <fp>` | Dismiss a finding by fingerprint |\n\
         | 👎 / `-1` on a finding | Learn dismiss (reaction) |\n\
         | `help` | Show this help |\n\n\
         <sub>Self-hosted · BYOK · fail-closed offline mode</sub>\n"
    )
}

fn escape_md(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ").replace('*', "\\*")
}

/// Heuristic 1–5 review effort estimate (no LLM).
fn estimate_review_effort(files: usize, findings: usize, blocking: usize) -> u8 {
    let mut score = 1u8;
    if files > 3 {
        score += 1;
    }
    if files > 12 {
        score += 1;
    }
    if findings > 0 {
        score += 1;
    }
    if blocking > 0 || findings > 8 {
        score += 1;
    }
    score.min(5)
}

fn effort_color(effort: u8) -> &'static str {
    match effort {
        1 => "22c55e",
        2 => "84cc16",
        3 => "f59e0b",
        4 => "f97316",
        _ => "e11d48",
    }
}

/// Grouped changed-files summary by top-level path area.
fn write_changed_files_summary(body: &mut String, files: &[serde_json::Value]) {
    use std::collections::BTreeMap;
    let _ = writeln!(body, "#### Changed files\n");
    if files.is_empty() {
        let _ = writeln!(body, "_No files in diff._\n");
        return;
    }

    let mut by_area: BTreeMap<String, Vec<(&str, &str)>> = BTreeMap::new();
    for file in files {
        let name = file["filename"].as_str().unwrap_or("?");
        let status = file["status"].as_str().unwrap_or("modified");
        let area = name
            .split('/')
            .next()
            .filter(|p| !p.is_empty() && *p != name)
            .unwrap_or("(root)")
            .to_string();
        by_area.entry(area).or_default().push((name, status));
    }

    let _ = writeln!(body, "| Area | Files | Status mix |");
    let _ = writeln!(body, "| --- | ---: | --- |");
    for (area, entries) in by_area.iter().take(12) {
        let mut added = 0usize;
        let mut modified = 0usize;
        let mut removed = 0usize;
        for (_, st) in entries {
            match *st {
                "added" => added += 1,
                "removed" => removed += 1,
                _ => modified += 1,
            }
        }
        let mix = format!("+{added} ~{modified} -{removed}");
        let _ = writeln!(body, "| `{area}` | {} | `{mix}` |", entries.len());
    }
    if by_area.len() > 12 {
        let _ = writeln!(body, "| _…_ | _{} areas_ | |", by_area.len() - 12);
    }
    let _ = writeln!(body);

    // Compact path list (capped) for reviewers who want the full set.
    let _ = writeln!(body, "<details>\n<summary>File list</summary>\n");
    let _ = writeln!(body, "| File | Status |");
    let _ = writeln!(body, "| --- | --- |");
    for file in files.iter().take(40) {
        let name = file["filename"].as_str().unwrap_or("?");
        let status = file["status"].as_str().unwrap_or("modified");
        let _ = writeln!(body, "| `{name}` | `{status}` |");
    }
    if files.len() > 40 {
        let _ = writeln!(body, "| _…_ | _{} more_ |", files.len() - 40);
    }
    let _ = writeln!(body, "\n</details>\n");
}

/// Mermaid change-map of top path prefixes (GitHub renders natively; no LLM).
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
    nodes.truncate(8);

    // Top-down map reads better in the walkthrough than a left-to-right chain.
    let mut out = String::from("flowchart TB\n  PR[Pull request] --> Review[Codasaurus]\n");
    for (i, (name, n)) in nodes.iter().enumerate() {
        let id = format!("N{i}");
        let _ = writeln!(out, "  Review --> {id}[{name} x{n}]");
    }
    Some(out)
}

fn truncate_utf8_owned(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }
    let mut idx = max_bytes.saturating_sub(40);
    while !s.is_char_boundary(idx) {
        idx -= 1;
    }
    s.truncate(idx);
    s.push_str("\n\n---\n_…truncated — re-run `@codasaurus review` for the full comment._");
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

    #[test]
    fn inline_includes_provenance_details() {
        let f = Finding {
            detector: "secrets".into(),
            severity: "blocking",
            file: "a.rs".into(),
            line: 1,
            column: 0,
            message: "key".into(),
            suggestion: None,
            evidence: Some("AKIA".into()),
            codemod: None,
        };
        let body = inline_finding_comment(&f);
        assert!(body.contains("<details>"));
        assert!(body.contains("Provenance"));
        assert!(body.contains("@codasaurus ignore"));
    }

    #[test]
    fn help_lists_impact() {
        assert!(help_body().contains("impact"));
        assert!(commands_details().contains("`@codasaurus impact`"));
    }

    #[test]
    fn mermaid_change_map_from_files() {
        let files = vec![
            serde_json::json!({"filename": "src/bot/mod.rs", "status": "modified"}),
            serde_json::json!({"filename": "src/api/setup.rs", "status": "modified"}),
            serde_json::json!({"filename": "docs/setup.md", "status": "added"}),
        ];
        let diagram = mermaid_change_flow(&files).expect("diagram");
        assert!(diagram.contains("flowchart TB"));
        assert!(diagram.contains("src"));
        assert!(diagram.contains("docs"));
    }

    #[test]
    fn effort_scales_with_size() {
        assert_eq!(estimate_review_effort(1, 0, 0), 1);
        assert!(estimate_review_effort(20, 10, 2) >= 4);
    }
}
