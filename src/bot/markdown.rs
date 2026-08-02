//! Readable PR comment markdown for GitHub review threads.
//!
//! Guide first: name what to do next, celebrate progress, keep machinery tucked away.

use crate::bot_runtime::BotRuntimeConfig;
use crate::config::Config;
use crate::detectors::{Finding, Findings};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

/// Codasaurus theme colors (dashboard tokens) for shields.io badges.
const LABEL_COLOR: &str = "1a1a1a";
const COLOR_SHIP: &str = "16a34a";
const COLOR_HOLD: &str = "dc2626";
const COLOR_REVIEW: &str = "d97706";
const COLOR_INFO: &str = "64748b";
const COLOR_MUTED: &str = "9a9a9a";
const COLOR_ACCENT: &str = "ff6659";

/// shields.io badge (flat-square) with dark label — safe for GitHub PR markdown.
fn shield(label: &str, message: &str, color: &str) -> String {
    let label_e = urlencoding_lite(label);
    let message_e = urlencoding_lite(message);
    format!(
        "<img src=\"https://img.shields.io/badge/{label_e}-{message_e}-{color}?style=flat-square&labelColor={LABEL_COLOR}\" alt=\"{label}: {message}\" />"
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

/// Soft severity cue for inline comments (no badge theater).
pub fn severity_badge(severity: &str) -> String {
    match severity {
        "blocking" => "**Needs fix**".into(),
        "warning" => "**Note**".into(),
        _ => "**FYI**".into(),
    }
}

pub fn ask_header() -> String {
    format!("### {} ask\n", shield("codasaurus", "ask", COLOR_ACCENT))
}

/// Short fingerprint for display (first 12 hex chars).
pub fn short_fp(finding: &Finding) -> String {
    let fp = finding.fingerprint();
    fp.chars().take(12).collect()
}

/// Shared compact commands footer for review / describe comments.
pub fn commands_details() -> String {
    "<details>\n<summary>Commands</summary>\n\n\
     Type as plain text (GitHub Apps are not @-mentionable):\n\n\
     `review` · `describe` · `summarize` · `improve` · `security` · `ask …` · `ignore <fp>` · `help`\n\n\
     <sub>Full list: <code>@codasaurus help</code></sub>\n\n\
     </details>\n"
        .into()
}

/// Inline finding comment — lead with the action; tuck bot machinery away.
pub fn inline_finding_comment(f: &Finding) -> String {
    let badge = severity_badge(f.severity);
    let title = finding_title(f);
    let why = finding_why(f);
    let action = finding_action(f);
    let fp = short_fp(f);
    let provenance = crate::bot::provenance::provenance_line(f);

    let mut body = format!(
        "{badge} · **{title}**\n\n\
         **Do this:** {action}\n\n\
         <details>\n<summary>Why it matters</summary>\n\n{why}\n\n{provenance}\n\n</details>\n"
    );

    if let Some(ref code) = f.codemod {
        let _ = write!(body, "\n```suggestion\n{}\n```\n", code.trim_end());
    }

    let _ = write!(
        body,
        "\n<details>\n<summary>Dismiss / fix commands</summary>\n\n\
         <code>fingerprint: {fp}</code>\n\n\
         `@codasaurus ignore {fp}` · 👎 to dismiss\
         {}{}\n\n</details>\n",
        if f.codemod.as_ref().is_some_and(|c| !c.is_empty()) {
            format!(" · `@codasaurus fix {fp}`")
        } else {
            String::new()
        },
        if f.codemod.as_ref().is_some_and(|c| !c.is_empty()) {
            " (needs Contents Write + allow_auto_fix)"
        } else {
            ""
        },
    );

    body
}

fn finding_title(f: &Finding) -> String {
    match f.detector.as_str() {
        "hallucinated-imports" => format!("Missing package `{}`", pkg(f)),
        "secrets" => "Secret in the code".into(),
        "phantom-deps" => format!("Import not listed in the project (`{}`)", pkg(f)),
        "todo-leaks" => "Unfinished TODO left in".into(),
        "vulnerabilities" => format!("Known vulnerability in `{}`", pkg_from_suggestion(f)),
        "over-engineering" => "Extra abstraction that may not be needed".into(),
        "boilerplate" => "Repeated code".into(),
        "stale-api" => "Older API still in use".into(),
        "graph" => "Code that looks unused".into(),
        "guidelines" => "Project guideline".into(),
        "slop" => "Signs of AI-generated filler".into(),
        "iac" => "Infra config risk".into(),
        "policy" => "Repo policy check".into(),
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

/// Optional sections for the split review comments.
#[derive(Debug, Clone, Default)]
pub struct WalkthroughExtras<'a> {
    pub related_prs: &'a [String],
    pub issue_assessment_md: &'a str,
    pub blast_md: &'a str,
    pub dep_delta_md: &'a str,
    /// Sanitized Mermaid `sequenceDiagram` body (no fences), if any.
    pub sequence_mermaid: Option<&'a str>,
    /// One short caption under the sequence diagram.
    pub sequence_caption: Option<&'a str>,
}

/// One item in a resolve / still-open / new narrative.
#[derive(Debug, Clone)]
pub struct ProgressItem {
    pub label: String,
    pub severity: String,
}

/// Diff of findings vs the previous completed review for this PR.
#[derive(Debug, Clone, Default)]
pub struct FindingProgress {
    pub resolved: Vec<ProgressItem>,
    pub still_open: Vec<ProgressItem>,
    pub newly_found: Vec<ProgressItem>,
}

impl FindingProgress {
    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty() && self.still_open.is_empty() && self.newly_found.is_empty()
    }
}

/// Strip `{review_id}:` prefix from persisted fingerprints.
pub fn raw_fingerprint(stored: &str) -> &str {
    if let Some((prefix, rest)) = stored.split_once(':') {
        if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) && rest.len() >= 32 {
            return rest;
        }
    }
    stored
}

/// Compare current findings to a prior review's stored findings.
pub fn compute_finding_progress(
    current: &[Finding],
    prior: &[(String, String, String)],
) -> Option<FindingProgress> {
    if prior.is_empty() {
        return None;
    }

    let mut prior_by_fp: HashMap<&str, (&str, &str)> = HashMap::new();
    for (fp, label, sev) in prior {
        prior_by_fp.insert(raw_fingerprint(fp), (label.as_str(), sev.as_str()));
    }
    let prior_fps: HashSet<&str> = prior_by_fp.keys().copied().collect();

    let mut current_by_fp: HashMap<String, ProgressItem> = HashMap::new();
    for f in current {
        let fp = f.fingerprint();
        current_by_fp.insert(
            fp,
            ProgressItem {
                label: guide_label(f),
                severity: f.severity.to_string(),
            },
        );
    }
    let current_fps: HashSet<&str> = current_by_fp.keys().map(String::as_str).collect();

    let mut progress = FindingProgress::default();
    for fp in prior_fps.difference(&current_fps) {
        if let Some((label, sev)) = prior_by_fp.get(fp) {
            progress.resolved.push(ProgressItem {
                label: (*label).to_string(),
                severity: (*sev).to_string(),
            });
        }
    }
    for fp in current_fps.intersection(&prior_fps) {
        if let Some(item) = current_by_fp.get(*fp) {
            progress.still_open.push(item.clone());
        }
    }
    for fp in current_fps.difference(&prior_fps) {
        if let Some(item) = current_by_fp.get(*fp) {
            progress.newly_found.push(item.clone());
        }
    }

    // Prefer blocking / warning order in each bucket.
    let rank = |s: &str| match s {
        "blocking" => 0,
        "warning" => 1,
        _ => 2,
    };
    for bucket in [
        &mut progress.resolved,
        &mut progress.still_open,
        &mut progress.newly_found,
    ] {
        bucket.sort_by(|a, b| {
            rank(&a.severity)
                .cmp(&rank(&b.severity))
                .then(a.label.cmp(&b.label))
        });
    }

    Some(progress)
}

fn guide_label(f: &Finding) -> String {
    let title = finding_title(f);
    if f.line > 0 {
        format!("{title} in `{}:{}`", f.file, f.line)
    } else {
        format!("{title} in `{}`", f.file)
    }
}

/// Short label for a prior DB finding (no live `Finding` struct).
pub fn guide_label_parts(detector: &str, message: &str, file: &str, line: Option<i32>) -> String {
    let stub = Finding {
        detector: detector.to_string(),
        severity: "info",
        file: file.to_string(),
        line: line.unwrap_or(0) as usize,
        column: 0,
        message: message.to_string(),
        suggestion: None,
        evidence: None,
        codemod: None,
    };
    guide_label(&stub)
}

fn write_slot_header(body: &mut String, marker: &str, title: &str) {
    let _ = writeln!(body, "<!-- {marker} -->\n");
    let _ = writeln!(body, "### {title}\n");
}

/// Caps shields strip for blast radius (same style as overview severity badges).
pub fn blast_radius_badges(score: u8) -> String {
    let (level, color) = match score {
        0..=24 => ("LOW", COLOR_SHIP),
        25..=59 => ("MODERATE", COLOR_REVIEW),
        60..=84 => ("HIGH", COLOR_HOLD),
        _ => ("CRITICAL", COLOR_HOLD),
    };
    format!(
        "{} {}\n",
        shield("BLAST RADIUS", level, color),
        shield(
            "SCORE",
            &score.to_string(),
            if score == 0 { COLOR_MUTED } else { color }
        ),
    )
}

/// Top-of-overview status strip: severity counts + ready-to-merge.
fn status_badge_strip(blocking: usize, warning: usize, info: usize, ready: bool) -> String {
    let blocking_color = if blocking > 0 {
        COLOR_HOLD
    } else {
        COLOR_MUTED
    };
    let warning_color = if warning > 0 {
        COLOR_REVIEW
    } else {
        COLOR_MUTED
    };
    let info_color = if info > 0 { COLOR_INFO } else { COLOR_MUTED };
    let (ready_msg, ready_color) = if ready {
        ("YES", COLOR_SHIP)
    } else {
        ("NO", COLOR_HOLD)
    };
    format!(
        "{} {} {} {}\n",
        shield("BLOCKING", &blocking.to_string(), blocking_color),
        shield("WARNING", &warning.to_string(), warning_color),
        shield("INFO", &info.to_string(), info_color),
        shield("READY TO MERGE", ready_msg, ready_color),
    )
}

/// Note shown in the overview when a PR title was suggested or auto-updated.
#[derive(Debug, Clone)]
pub struct TitleFixNote {
    pub proposed: String,
    pub applied: bool,
}

/// Message 1: status badges + prose + what-to-do checklist + progress + Changes.
#[allow(clippy::too_many_arguments)]
pub fn overview_comment_body(
    findings: &Findings,
    has_blocking: bool,
    pr_title: &str,
    files: &[serde_json::Value],
    runtime: &BotRuntimeConfig,
    agent_badge: Option<&str>,
    progress: Option<&FindingProgress>,
    title_fix: Option<&TitleFixNote>,
) -> String {
    let counts = findings.count_by_severity();
    let blocking = *counts.get("blocking").unwrap_or(&0);
    let warning = *counts.get("warning").unwrap_or(&0);
    let info = *counts.get("info").unwrap_or(&0);
    let total = findings.findings.len();
    let ready = !has_blocking && blocking == 0;

    let mut body = String::with_capacity(1536);
    write_slot_header(&mut body, "codasaurus:overview:v1", "Codasaurus");
    body.push_str(&status_badge_strip(blocking, warning, info, ready));
    body.push('\n');
    if let Some(badge) = agent_badge.filter(|s| !s.is_empty()) {
        let _ = writeln!(body, "{badge}\n");
    }
    let _ = writeln!(
        body,
        "{}\n",
        walkthrough_prose(pr_title, files, total, blocking, warning, has_blocking)
    );

    if let Some(p) = progress.filter(|p| !p.is_empty()) {
        write_progress_section(&mut body, p);
    }

    write_next_actions(&mut body, &findings.findings);
    write_title_fix_note(&mut body, title_fix);
    write_agent_fix_prompt(&mut body, &findings.findings, pr_title);

    let _ = writeln!(body, "## Changes\n");
    write_changed_files_table(&mut body, files);
    if body.len() > runtime.max_comment_bytes {
        truncate_utf8_owned(&mut body, runtime.max_comment_bytes);
    }
    body
}

fn write_title_fix_note(body: &mut String, note: Option<&TitleFixNote>) {
    let Some(note) = note else {
        return;
    };
    let title = redact_secrets(&note.proposed);
    if note.applied {
        let _ = writeln!(
            body,
            "> Updated PR title to `{title}` (high-confidence title fix).\n"
        );
    } else {
        let _ = writeln!(
            body,
            "> Suggested title: `{title}` — rename the PR if this looks right.\n"
        );
    }
}

fn write_next_actions(body: &mut String, findings: &[Finding]) {
    if findings.is_empty() {
        return;
    }
    let ordered = ordered_checklist_findings(findings);
    let _ = writeln!(body, "## What to do next\n");
    for (i, f) in ordered.iter().take(5).enumerate() {
        let cue = match f.severity {
            "blocking" => "Please fix",
            "warning" => "Please check",
            _ => "Worth a look",
        };
        let _ = writeln!(body, "{}. **{cue}:** {}", i + 1, guide_label(f));
    }
    if ordered.len() > 5 {
        let _ = writeln!(
            body,
            "\n_{} more on the Files tab (inline comments)._",
            ordered.len() - 5
        );
    }
    body.push('\n');
}

/// Ordered findings for checklist / agent prompt (blocking first, max 5).
fn ordered_checklist_findings(findings: &[Finding]) -> Vec<&Finding> {
    let mut ordered: Vec<&Finding> = findings.iter().collect();
    ordered.sort_by_key(|f| match f.severity {
        "blocking" => 0,
        "warning" => 1,
        _ => 2,
    });
    ordered
}

/// Deterministic copy-paste brief for an AI coding agent. `None` when clean.
pub fn agent_fix_prompt(findings: &[Finding], pr_title: &str) -> Option<String> {
    if findings.is_empty() {
        return None;
    }
    let ordered = ordered_checklist_findings(findings);
    let shown = ordered.iter().take(5).copied().collect::<Vec<_>>();
    if shown.is_empty() {
        return None;
    }

    let mut prompt = String::with_capacity(1024);
    prompt.push_str(
        "You are fixing review findings on this pull request. Change only what is needed. \
Do not refactor unrelated code. Do not invent new features. Prefer the smallest diff. \
If a finding looks wrong, say so instead of changing code. After edits, summarize what you changed.\n\n\
Findings to fix:\n",
    );

    for (i, f) in shown.iter().enumerate() {
        let action = redact_secrets(&finding_action(f));
        let _ = writeln!(
            prompt,
            "{}. [{}] {}\n   Do this: {}",
            i + 1,
            f.severity,
            guide_label(f),
            action
        );
    }
    if ordered.len() > 5 {
        let _ = writeln!(
            prompt,
            "\n({} more findings are on the PR Files tab; fix the list above first.)",
            ordered.len() - 5
        );
    }

    let title = pr_title.trim();
    if !title.is_empty() {
        let _ = write!(
            prompt,
            "\nPR title: {}",
            redact_secrets(&title.chars().take(200).collect::<String>())
        );
    }

    // Keep the overview comment lean.
    const MAX_CHARS: usize = 2800;
    if prompt.chars().count() > MAX_CHARS {
        prompt = prompt.chars().take(MAX_CHARS.saturating_sub(1)).collect();
        prompt.push('…');
    }
    Some(prompt)
}

fn write_agent_fix_prompt(body: &mut String, findings: &[Finding], pr_title: &str) {
    let Some(prompt) = agent_fix_prompt(findings, pr_title) else {
        return;
    };
    let _ = writeln!(
        body,
        "<details>\n<summary>Copy this into your AI coding agent</summary>\n\n\
         Paste into your AI coding agent to apply the fixes:\n\n\
         ```text\n{prompt}\n```\n\n</details>\n"
    );
}

fn write_progress_section(body: &mut String, progress: &FindingProgress) {
    let _ = writeln!(body, "## Since last review\n");
    if !progress.resolved.is_empty() {
        let _ = writeln!(body, "**Resolved**");
        for item in progress.resolved.iter().take(5) {
            let _ = writeln!(body, "- ~~{}~~", escape_md(&item.label));
        }
        body.push('\n');
    }
    if !progress.still_open.is_empty() {
        let _ = writeln!(body, "**Still open**");
        for item in progress.still_open.iter().take(5) {
            let _ = writeln!(body, "- {}", escape_md(&item.label));
        }
        body.push('\n');
    }
    if !progress.newly_found.is_empty() {
        let _ = writeln!(body, "**New**");
        for item in progress.newly_found.iter().take(5) {
            let _ = writeln!(body, "- {}", escape_md(&item.label));
        }
        body.push('\n');
    }
}

/// Message 2: sequence diagram + blast / related / deps. `None` when empty.
pub fn context_comment_body(
    extras: &WalkthroughExtras<'_>,
    runtime: &BotRuntimeConfig,
) -> Option<String> {
    let has_seq = extras
        .sequence_mermaid
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_blast = !extras.blast_md.trim().is_empty();
    let has_dep = !extras.dep_delta_md.trim().is_empty();
    let has_related = !extras.related_prs.is_empty();
    let has_issues = !extras.issue_assessment_md.trim().is_empty();
    if !has_seq && !has_blast && !has_dep && !has_related && !has_issues {
        return None;
    }

    let mut body = String::with_capacity(1024);
    write_slot_header(&mut body, "codasaurus:context:v1", "Context");

    if has_seq {
        let _ = writeln!(body, "## Flow\n");
        let _ = writeln!(
            body,
            "```mermaid\n{}\n```\n",
            extras.sequence_mermaid.unwrap_or("").trim()
        );
        if let Some(cap) = extras.sequence_caption.filter(|s| !s.trim().is_empty()) {
            let _ = writeln!(body, "_{}_\n", escape_md(cap.trim()));
        }
    }
    if has_issues {
        body.push_str(extras.issue_assessment_md.trim());
        body.push_str("\n\n");
    }
    if has_blast {
        let _ = writeln!(body, "## Blast radius\n");
        body.push_str(extras.blast_md.trim());
        body.push_str("\n\n");
    }
    if has_dep {
        let _ = writeln!(body, "## Dependencies\n");
        body.push_str(extras.dep_delta_md.trim());
        body.push_str("\n\n");
    }
    if has_related {
        let _ = writeln!(body, "## Related pull requests\n");
        for r in extras.related_prs.iter().take(3) {
            let _ = writeln!(body, "- {r}");
        }
        body.push('\n');
    }

    if body.len() > runtime.max_comment_bytes {
        truncate_utf8_owned(&mut body, runtime.max_comment_bytes);
    }
    Some(body)
}

/// One-line stub so a prior context comment does not keep a stale diagram.
pub fn context_comment_stub() -> String {
    "<!-- codasaurus:context:v1 -->\n\n_No extra context for this change._\n".into()
}

/// Message 3: pre-merge guidance. Quiet when green; teaches when unchecked.
pub fn checks_comment_body(
    findings: &Findings,
    has_blocking: bool,
    pr_title: &str,
    files: &[serde_json::Value],
    reviewers: &[String],
    config: &Config,
    runtime: &BotRuntimeConfig,
) -> String {
    let counts = findings.count_by_severity();
    let blocking = *counts.get("blocking").unwrap_or(&0);
    let warning = *counts.get("warning").unwrap_or(&0);
    let total = findings.findings.len();
    let title_ok = !pr_title.trim().is_empty();
    let warnings_ok = warning <= config.pre_merge.max_warnings;
    let all_clear = !has_blocking && warnings_ok && title_ok && total == 0;

    let mut body = String::with_capacity(512);
    write_slot_header(&mut body, "codasaurus:checks:v1", "Checks");

    if all_clear {
        let _ = writeln!(
            body,
            "All clear. Automated checks did not find anything to fix before merge.\n"
        );
        if !reviewers.is_empty() {
            let _ = writeln!(
                body,
                "Suggested eyes: {}\n",
                reviewers
                    .iter()
                    .take(4)
                    .map(|r| format!("@{r}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        body.push_str(&commands_details());
        if body.len() > runtime.max_comment_bytes {
            truncate_utf8_owned(&mut body, runtime.max_comment_bytes);
        }
        return body;
    }

    let _ = writeln!(body, "## Before merge\n");

    let warn_hint = format!(
        "At most {} warning{} allowed. Clear the notes on the Files tab, or ask a teammate.",
        config.pre_merge.max_warnings,
        if config.pre_merge.max_warnings == 1 {
            ""
        } else {
            "s"
        }
    );
    let checks = [
        (
            "No blocking findings",
            !has_blocking,
            "Work through **What to do next** (items marked **Please fix**), then push again.",
        ),
        (
            "Warning budget within limit",
            warnings_ok,
            warn_hint.as_str(),
        ),
        (
            "PR has a title",
            title_ok,
            "Add a short title that says what this PR changes.",
        ),
    ];
    for (label, ok, hint) in checks {
        let mark = if ok { "x" } else { " " };
        let _ = writeln!(body, "- [{mark}] **{label}**");
        if !ok {
            let _ = writeln!(body, "  - {hint}");
        }
    }
    body.push('\n');

    let effort = estimate_review_effort(files.len(), total, blocking);
    let (effort_label, minutes) = effort_label_and_minutes(effort);
    let _ = writeln!(
        body,
        "Reviewer time ~{minutes} min ({effort}/5 · {effort_label} · {} file{})\n",
        files.len(),
        if files.len() == 1 { "" } else { "s" },
    );

    if !reviewers.is_empty() {
        let _ = writeln!(
            body,
            "Suggested eyes: {}\n",
            reviewers
                .iter()
                .take(4)
                .map(|r| format!("@{r}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }

    body.push_str(&commands_details());
    if body.len() > runtime.max_comment_bytes {
        truncate_utf8_owned(&mut body, runtime.max_comment_bytes);
    }
    body
}

/// Sanitize LLM Mermaid sequence output. Returns `None` if unusable.
pub fn sanitize_sequence_mermaid(raw: &str) -> Option<(String, Option<String>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("none")
        || trimmed.eq_ignore_ascii_case("abstain")
    {
        return None;
    }

    // Strip optional fences / leading prose.
    let mut mermaid = trimmed.to_string();
    if let Some(idx) = mermaid.find("```") {
        let after = &mermaid[idx + 3..];
        let after = after.strip_prefix("mermaid").unwrap_or(after).trim_start();
        if let Some(end) = after.find("```") {
            mermaid = after[..end].trim().to_string();
        } else {
            mermaid = after.trim().to_string();
        }
    }
    if !mermaid.to_ascii_lowercase().contains("sequencediagram") {
        // Allow body-only; prepend header.
        if mermaid
            .lines()
            .any(|l| l.contains("->>") || l.contains("-->>"))
        {
            mermaid = format!("sequenceDiagram\n{mermaid}");
        } else {
            return None;
        }
    }

    let reserved = [
        "loop",
        "alt",
        "opt",
        "par",
        "and",
        "end",
        "note",
        "rect",
        "critical",
        "break",
        "participant",
        "actor",
    ];
    let mut lines_out = Vec::new();
    let mut participants = 0usize;
    let mut arrows = 0usize;
    for line in mermaid.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("participant ") || lower.starts_with("actor ") {
            participants += 1;
            if participants > 6 {
                continue;
            }
            let rest = t
                .split_whitespace()
                .nth(1)
                .unwrap_or("P")
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
            let id = if rest.is_empty() || reserved.iter().any(|r| rest.eq_ignore_ascii_case(r)) {
                format!("P{participants}")
            } else {
                rest.chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .take(24)
                    .collect::<String>()
            };
            let label = t
                .split_once(" as ")
                .map(|(_, l)| l.trim())
                .filter(|l| !l.is_empty())
                .unwrap_or(rest);
            let label = label.chars().take(32).collect::<String>();
            lines_out.push(format!("  participant {id} as {label}"));
            continue;
        }
        if t.contains("->>") || t.contains("-->>") || t.contains("->") {
            arrows += 1;
            if arrows > 10 {
                continue;
            }
            lines_out.push(format!("  {t}"));
            continue;
        }
        if lower == "sequencediagram" {
            lines_out.push("sequenceDiagram".into());
            continue;
        }
        // Keep simple alt/opt/end blocks but cap noise.
        if lower.starts_with("alt ")
            || lower.starts_with("opt ")
            || lower.starts_with("else")
            || lower == "end"
            || lower.starts_with("note ")
        {
            lines_out.push(format!("  {t}"));
        }
    }
    if arrows == 0 {
        return None;
    }
    if !lines_out.iter().any(|l| l == "sequenceDiagram") {
        lines_out.insert(0, "sequenceDiagram".into());
    }
    Some((lines_out.join("\n"), None))
}

/// True when a sequence diagram is worth attempting (≥2 high-signal source files).
pub fn should_attempt_sequence_diagram(paths: &[String]) -> bool {
    let high = paths
        .iter()
        .filter(|p| !crate::llm::is_low_signal_path(p))
        .count();
    high >= 2
}

/// One or two short sentences summarizing the PR (≤~280 chars).
fn walkthrough_prose(
    pr_title: &str,
    files: &[serde_json::Value],
    total: usize,
    blocking: usize,
    warning: usize,
    has_blocking: bool,
) -> String {
    use std::collections::BTreeMap;
    let title = pr_title.trim();
    let mut by_area: BTreeMap<String, usize> = BTreeMap::new();
    for file in files {
        let name = file["filename"].as_str().unwrap_or("?");
        let area = name
            .split('/')
            .next()
            .filter(|p| !p.is_empty() && *p != name)
            .unwrap_or("(root)")
            .to_string();
        *by_area.entry(area).or_default() += 1;
    }
    let mut areas: Vec<(String, usize)> = by_area.into_iter().collect();
    areas.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top: Vec<String> = areas
        .iter()
        .take(3)
        .map(|(a, _)| format!("`{a}`"))
        .collect();

    let mut s = String::new();
    if !title.is_empty() {
        s.push_str(&format!("**{}**\n\n", escape_md(title)));
    }
    if files.is_empty() {
        s.push_str("No file changes showed up in this review.");
    } else if top.len() <= 1 {
        s.push_str(&format!(
            "This PR updates {} file{} (mostly {}).",
            files.len(),
            if files.len() == 1 { "" } else { "s" },
            top.first().map(String::as_str).unwrap_or("the repo")
        ));
    } else {
        s.push_str(&format!(
            "This PR updates {} files across {}.",
            files.len(),
            top.join(", ")
        ));
    }
    if total == 0 {
        s.push_str(" Automated checks look clear.");
    } else if has_blocking || blocking > 0 {
        s.push_str(&format!(
            " I found {} thing{} to fix before merge. Start with the list below.",
            blocking,
            if blocking == 1 { "" } else { "s" }
        ));
    } else if warning > 0 {
        s.push_str(&format!(
            " I spotted {} thing{} worth a quick look (not blockers). See the list below.",
            warning,
            if warning == 1 { "" } else { "s" }
        ));
    } else {
        s.push_str(" A few small notes are listed below.");
    }
    if s.chars().count() > 280 {
        s = s.chars().take(277).collect::<String>() + "…";
    }
    s
}

pub fn help_body() -> String {
    let brand = shield("codasaurus", "commands", COLOR_ACCENT);
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

fn effort_label_and_minutes(effort: u8) -> (&'static str, u16) {
    match effort {
        1 => ("Trivial", 2),
        2 => ("Simple", 10),
        3 => ("Moderate", 20),
        4 => ("Complex", 35),
        _ => ("Critical", 50),
    }
}

/// Compact Changes table (≤8 rows) + full file list in details.
fn write_changed_files_table(body: &mut String, files: &[serde_json::Value]) {
    if files.is_empty() {
        let _ = writeln!(body, "_No files in diff._\n");
        return;
    }

    let _ = writeln!(body, "| Path | What changed |\n| --- | --- |");
    for file in files.iter().take(8) {
        let name = file["filename"].as_str().unwrap_or("?");
        let status = file["status"].as_str().unwrap_or("modified");
        let what = match status {
            "added" => "added",
            "removed" | "deleted" => "removed",
            "renamed" => "renamed",
            _ => "updated",
        };
        let short = if name.len() > 48 {
            format!("…{}", &name[name.len().saturating_sub(45)..])
        } else {
            name.to_string()
        };
        let _ = writeln!(body, "| `{short}` | {what} |");
    }
    if files.len() > 8 {
        let _ = writeln!(body, "| _…{} more_ | |", files.len() - 8);
    }
    let _ = writeln!(body);

    let _ = writeln!(
        body,
        "<details>\n<summary>All files ({})</summary>\n",
        files.len()
    );
    for file in files.iter().take(40) {
        let name = file["filename"].as_str().unwrap_or("?");
        let status = file["status"].as_str().unwrap_or("modified");
        let _ = writeln!(body, "- `{name}` ({status})");
    }
    if files.len() > 40 {
        let _ = writeln!(body, "- _…{} more_", files.len() - 40);
    }
    let _ = writeln!(body, "\n</details>\n");
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
        assert!(body.contains("Why it matters"));
        assert!(body.contains("**Do this:**"));
        assert!(body.contains("@codasaurus ignore"));
    }

    #[test]
    fn help_lists_impact() {
        assert!(help_body().contains("review"));
        assert!(commands_details().contains("@codasaurus help"));
    }

    #[test]
    fn overview_is_lean_with_changes_table() {
        let findings = Findings {
            findings: vec![Finding {
                detector: "boilerplate".into(),
                severity: "warning",
                file: "src/bot/commands.rs".into(),
                line: 10,
                column: 0,
                message: "dup".into(),
                suggestion: None,
                evidence: None,
                codemod: None,
            }],
        };
        let files = vec![
            serde_json::json!({"filename": "src/bot/mod.rs", "status": "modified"}),
            serde_json::json!({"filename": "CHANGELOG.md", "status": "modified"}),
        ];
        let runtime = BotRuntimeConfig::default();
        let body = overview_comment_body(
            &findings,
            false,
            "Stop stacking comments",
            &files,
            &runtime,
            None,
            None,
            None,
        );
        assert!(body.contains("<!-- codasaurus:overview:v1 -->"));
        assert!(body.contains("## What to do next"));
        assert!(body.contains("src/bot/commands.rs"));
        assert!(body.contains("## Changes"));
        assert!(body.contains("| Path | What changed |"));
        assert!(body.contains("shields.io"));
        assert!(body.contains("alt=\"BLOCKING:"));
        assert!(body.contains("alt=\"WARNING:"));
        assert!(body.contains("alt=\"INFO:"));
        assert!(body.contains("alt=\"READY TO MERGE:"));
        assert!(!body.contains("review-banner"));
        assert!(!body.contains("## Review effort"));
        assert!(!body.contains("Pre-merge"));
        assert!(!body.contains("flowchart TB"));
        assert!(body.contains("Copy this into your AI coding agent"));
        assert!(body.contains("```text"));
        assert!(body.contains("Findings to fix:"));
        assert!(body.contains("Do this:"));

        let checks = checks_comment_body(
            &findings,
            false,
            "Stop stacking comments",
            &files,
            &["alice".into()],
            &Config::default(),
            &runtime,
        );
        assert!(checks.contains("## Before merge") || checks.contains("All clear"));
        assert!(checks.contains("@alice"));

        let clean = checks_comment_body(
            &Findings::default(),
            false,
            "Clean PR",
            &files,
            &[],
            &Config::default(),
            &runtime,
        );
        assert!(clean.contains("All clear"));
        assert!(!clean.contains("## Before merge"));

        let clean_overview = overview_comment_body(
            &Findings::default(),
            false,
            "Clean PR",
            &files,
            &runtime,
            None,
            None,
            None,
        );
        assert!(!clean_overview.contains("Copy this into your AI coding agent"));
        assert!(!clean_overview.contains("Findings to fix:"));

        let progress = FindingProgress {
            resolved: vec![ProgressItem {
                label: "Old issue in `a.rs:1`".into(),
                severity: "blocking".into(),
            }],
            still_open: vec![ProgressItem {
                label: "Repeated code in `src/bot/commands.rs:10`".into(),
                severity: "warning".into(),
            }],
            newly_found: vec![],
        };
        let with_progress = overview_comment_body(
            &findings,
            false,
            "Stop stacking comments",
            &files,
            &runtime,
            None,
            Some(&progress),
            None,
        );
        assert!(with_progress.contains("## Since last review"));
        assert!(with_progress.contains("**Resolved**"));
        assert!(with_progress.contains("**Still open**"));

        let suggested = overview_comment_body(
            &Findings::default(),
            false,
            "update",
            &files,
            &runtime,
            None,
            None,
            Some(&TitleFixNote {
                proposed: "feat: add title fix".into(),
                applied: false,
            }),
        );
        assert!(suggested.contains("Suggested title: `feat: add title fix`"));
        let applied = overview_comment_body(
            &Findings::default(),
            false,
            "feat: add title fix",
            &files,
            &runtime,
            None,
            None,
            Some(&TitleFixNote {
                proposed: "feat: add title fix".into(),
                applied: true,
            }),
        );
        assert!(applied.contains("Updated PR title to `feat: add title fix`"));
        assert!(!applied.contains("Suggested title:"));

        let ctx = context_comment_body(
            &WalkthroughExtras {
                related_prs: &["#1: Example (2 shared files)".into()],
                issue_assessment_md: "## Linked issues\n\n- #2: Login timeout (looks covered)\n",
                blast_md: "blast",
                ..Default::default()
            },
            &runtime,
        )
        .expect("context");
        assert!(ctx.contains("## Related pull requests"));
        assert!(ctx.contains("## Linked issues"));
        assert!(ctx.contains("#2: Login timeout"));
        assert!(ctx.contains("## Blast radius"));
        assert!(context_comment_body(&WalkthroughExtras::default(), &runtime).is_none());
    }

    #[test]
    fn agent_fix_prompt_redacts_and_skips_clean() {
        assert!(agent_fix_prompt(&[], "x").is_none());

        let findings = vec![Finding {
            detector: "secrets".into(),
            severity: "blocking",
            file: "src/serve.rs".into(),
            line: 88,
            column: 0,
            message: "API key".into(),
            suggestion: Some("Rotate abcdefghijklmnopqrstuvwxyz0123456789 and use env".into()),
            evidence: None,
            codemod: None,
        }];
        let prompt = agent_fix_prompt(&findings, "Add webhook retries").expect("prompt");
        assert!(prompt.contains("Findings to fix:"));
        assert!(prompt.contains("src/serve.rs:88"));
        assert!(prompt.contains("[blocking]"));
        assert!(prompt.contains("PR title: Add webhook retries"));
        assert!(prompt.contains("redacted") || prompt.contains("…[redacted]"));
        assert!(!prompt.contains("abcdefghijklmnopqrstuvwxyz0123456789"));
        assert!(!prompt.contains(" — "));
    }

    #[test]
    fn progress_set_diff() {
        let a = Finding {
            detector: "secrets".into(),
            severity: "blocking",
            file: "a.rs".into(),
            line: 1,
            column: 0,
            message: "key".into(),
            suggestion: None,
            evidence: None,
            codemod: None,
        };
        let b = Finding {
            detector: "todo-leaks".into(),
            severity: "warning",
            file: "b.rs".into(),
            line: 2,
            column: 0,
            message: "TODO".into(),
            suggestion: None,
            evidence: None,
            codemod: None,
        };
        let prior = vec![
            (
                format!("9:{}", a.fingerprint()),
                guide_label(&a),
                "blocking".into(),
            ),
            (
                format!("9:{}", b.fingerprint()),
                guide_label(&b),
                "warning".into(),
            ),
        ];
        let current = vec![b.clone()];
        let p = compute_finding_progress(&current, &prior).expect("progress");
        assert_eq!(p.resolved.len(), 1);
        assert!(p.resolved[0].label.contains("Secret"));
        assert_eq!(p.still_open.len(), 1);
        assert!(p.newly_found.is_empty());
    }

    #[test]
    fn inline_leads_with_action() {
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
        assert!(body.contains("**Do this:**"));
        assert!(body.contains("Needs fix"));
        assert!(body.contains("Dismiss / fix commands"));
        assert!(!body.contains("shields.io"));
        assert!(body.contains("fingerprint:"));
    }

    #[test]
    fn sanitize_sequence_accepts_arrows() {
        let raw = "```mermaid\nsequenceDiagram\nparticipant Client\nparticipant API\nClient->>API: GET /x\n```";
        let (m, _) = sanitize_sequence_mermaid(raw).expect("ok");
        assert!(m.contains("sequenceDiagram"));
        assert!(m.contains("Client->>API"));
        assert!(!m.to_ascii_lowercase().contains("flowchart"));
    }

    #[test]
    fn sanitize_renames_reserved_participant() {
        let raw = "sequenceDiagram\nparticipant loop\nparticipant API\nloop->>API: x\n";
        let (m, _) = sanitize_sequence_mermaid(raw).expect("ok");
        assert!(!m.lines().any(|l| l.trim() == "participant loop"));
    }

    #[test]
    fn effort_scales_with_size() {
        assert_eq!(estimate_review_effort(1, 0, 0), 1);
        assert!(estimate_review_effort(20, 10, 2) >= 4);
    }
}
