//! Deterministic high-confidence PR title proposals (no LLM).
//!
//! Titles follow common PR / conventional-commit practices:
//! imperative mood, lowercase description, no trailing punctuation,
//! single line, ~72 char soft cap (100 hard), grounded in commits/paths.

use std::sync::LazyLock;

use regex::Regex;

use crate::context::guidelines::GuidelineFile;
use crate::context::rules::ExtractedRule;

static CONVENTIONAL_COMMIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(feat|fix|chore|docs|style|refactor|perf|test|build|ci|revert)(\([^)]+\))?!?: .+")
        .expect("invalid conventional commit regex")
});

static CONVENTIONAL_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<prefix>(feat|fix|chore|docs|style|refactor|perf|test|build|ci|revert)(\([^)]+\))?!?:)\s*(?P<body>.+)$")
        .expect("invalid conventional prefix regex")
});

static GENERIC_TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(fix(\s+some\s+issues)?|various\s+changes|improve\s+code|update|changes|wip|fix|chore|misc|stuff)\.?$")
        .expect("invalid generic title regex")
});

/// Soft subject length (conventional-commit / git log readability).
const SOFT_TITLE_CHARS: usize = 72;
/// Absolute cap after redaction.
const MAX_TITLE_CHARS: usize = 100;
const MIN_MEANINGFUL_CHARS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedTitle {
    pub title: String,
    /// Safe for `pr_title_fix=auto` (grounded in a conventional commit or guidelines+body).
    pub auto_safe: bool,
}

/// Whether contribution guidelines ask for conventional commits.
pub fn guidelines_want_conventional(files: &[GuidelineFile]) -> bool {
    files.iter().any(|gf| {
        gf.rules.iter().any(|r| match r {
            ExtractedRule::CommitRule { description } => {
                let d = description.to_lowercase();
                d.contains("conventional") || d.contains("type(scope)")
            }
            _ => false,
        })
    })
}

pub fn is_conventional_subject(subject: &str) -> bool {
    CONVENTIONAL_COMMIT_RE.is_match(subject.trim())
}

fn subject_line(msg: &str) -> &str {
    msg.lines().next().unwrap_or(msg).trim()
}

fn is_generic_title(title: &str) -> bool {
    let t = title.trim();
    if t.chars().count() < MIN_MEANINGFUL_CHARS {
        return true;
    }
    GENERIC_TITLE_RE.is_match(t)
        || matches!(
            t.to_ascii_lowercase().as_str(),
            "update" | "changes" | "fix" | "wip" | "chore" | "misc"
        )
}

fn is_noise_subject(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.starts_with("merge ")
        || l.starts_with("merge branch")
        || l.starts_with("merge pull request")
        || l.starts_with("revert \"")
        || l == "initial commit"
}

/// True when the current title should be improved.
pub fn title_needs_fix(current: &str, guidelines_want_conventional: bool) -> bool {
    let t = current.trim();
    if t.is_empty() || is_generic_title(t) {
        return true;
    }
    if guidelines_want_conventional && !is_conventional_subject(t) {
        return true;
    }
    // Already conventional but violates length / trailing punctuation — polish.
    if is_conventional_subject(t) && needs_polish(t) {
        return true;
    }
    false
}

fn needs_polish(title: &str) -> bool {
    let t = title.trim();
    if t.ends_with('.') || t.ends_with('!') || t.contains('\n') {
        return true;
    }
    if t.chars().count() > SOFT_TITLE_CHARS {
        return true;
    }
    if let Some(caps) = CONVENTIONAL_PREFIX_RE.captures(t) {
        let body = caps.name("body").map(|m| m.as_str()).unwrap_or("");
        if body
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase() && c.is_ascii_alphabetic())
        {
            return true;
        }
    }
    false
}

fn infer_type_from_paths(paths: &[String]) -> &'static str {
    if paths.is_empty() {
        return "chore";
    }
    let all_docs = paths.iter().all(|p| {
        let l = p.to_ascii_lowercase();
        l.starts_with("docs/")
            || l.ends_with(".md")
            || l.contains("/docs/")
            || l == "readme.md"
            || l == "changelog.md"
    });
    if all_docs {
        return "docs";
    }
    let all_tests = paths.iter().all(|p| {
        let l = p.to_ascii_lowercase();
        l.contains("/test/")
            || l.starts_with("tests/")
            || l.starts_with("test/")
            || l.contains("/tests/")
            || l.ends_with("_test.rs")
            || l.ends_with(".test.ts")
            || l.ends_with(".spec.ts")
            || l.ends_with("_test.go")
            || l.ends_with("_test.py")
    });
    if all_tests {
        return "test";
    }
    // Directory signals only — avoid matching filenames like `title_fix.rs`.
    let looks_fix = paths.iter().any(|p| {
        let l = p.to_ascii_lowercase();
        l.starts_with("fix/")
            || l.contains("/fix/")
            || l.starts_with("fixes/")
            || l.contains("/fixes/")
            || l.starts_with("bug/")
            || l.contains("/bug/")
            || l.starts_with("bugs/")
            || l.contains("/bugs/")
            || l.starts_with("hotfix/")
            || l.contains("/hotfix/")
    });
    if looks_fix {
        return "fix";
    }
    let looks_feat = paths.iter().any(|p| {
        let l = p.to_ascii_lowercase();
        l.starts_with("src/") || l.starts_with("app/") || l.starts_with("lib/")
    });
    if looks_feat {
        return "feat";
    }
    "chore"
}

/// Imperative / present-tense lead-ins → conventional subject style.
fn to_imperative(body: &str) -> String {
    let pairs = [
        ("added ", "add "),
        ("adds ", "add "),
        ("adding ", "add "),
        ("fixed ", "fix "),
        ("fixes ", "fix "),
        ("fixing ", "fix "),
        ("updated ", "update "),
        ("updates ", "update "),
        ("updating ", "update "),
        ("removed ", "remove "),
        ("removes ", "remove "),
        ("removing ", "remove "),
        ("implemented ", "implement "),
        ("implements ", "implement "),
        ("implementing ", "implement "),
        ("created ", "create "),
        ("creates ", "create "),
        ("creating ", "create "),
        ("refactored ", "refactor "),
        ("refactors ", "refactor "),
        ("improved ", "improve "),
        ("improves ", "improve "),
        ("changed ", "change "),
        ("changes ", "change "),
        ("renamed ", "rename "),
        ("renames ", "rename "),
        ("deleted ", "delete "),
        ("deletes ", "delete "),
        ("introduced ", "introduce "),
        ("introduces ", "introduce "),
    ];
    let lower = body.to_ascii_lowercase();
    for (from, to) in pairs {
        if lower.starts_with(from) {
            return format!("{to}{}", &body[from.len()..]);
        }
    }
    body.to_string()
}

/// Clean description body: no type prefix, imperative, lowercase start, no trailing punct.
fn clean_body(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    s = s.replace(['\n', '\r', '\t'], " ");
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s = s
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`'))
        .to_string();

    // Strip conventional prefix if present so we can rebuild cleanly.
    if let Some(caps) = CONVENTIONAL_PREFIX_RE.captures(&s) {
        s = caps
            .name("body")
            .map(|m| m.as_str().to_string())
            .unwrap_or(s);
    } else if let Some((head, rest)) = s.split_once(": ") {
        if head.len() < 40
            && head
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '(' | ')' | '!' | '/' | '-'))
        {
            s = rest.trim().to_string();
        }
    }

    s = s
        .trim_matches(|c: char| matches!(c, '.' | '!' | '?' | ',' | ';' | ':'))
        .to_string();
    s = to_imperative(&s);
    // Conventional descriptions are lowercase (keep ASCII letters lower).
    s = s.to_ascii_lowercase();
    s
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    // Prefer breaking at a word boundary when soft-truncating.
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    if let Some(idx) = out.rfind(' ') {
        if idx >= max / 2 {
            out.truncate(idx);
        }
    }
    out.push('…');
    out
}

fn finalize_title(s: &str) -> String {
    let redacted = crate::bot::markdown::redact_secrets(s.trim());
    let soft = truncate_chars(&redacted, SOFT_TITLE_CHARS);
    truncate_chars(&soft, MAX_TITLE_CHARS)
}

/// Polish a full subject (with or without conventional prefix) to best-practice form.
fn polish_subject(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(caps) = CONVENTIONAL_PREFIX_RE.captures(trimmed) {
        let prefix = caps.name("prefix").map(|m| m.as_str()).unwrap_or("");
        let body = clean_body(caps.name("body").map(|m| m.as_str()).unwrap_or(""));
        if body.is_empty() {
            return finalize_title(trimmed);
        }
        return finalize_title(&format!("{prefix} {body}"));
    }
    finalize_title(&clean_body(trimmed))
}

fn first_conventional_commit(commits: &[String]) -> Option<&str> {
    commits.iter().map(|c| subject_line(c)).find(|s| {
        is_conventional_subject(s) && s.len() >= MIN_MEANINGFUL_CHARS && !is_noise_subject(s)
    })
}

fn first_meaningful_commit(commits: &[String]) -> Option<&str> {
    commits.iter().map(|c| subject_line(c)).find(|s| {
        !s.is_empty()
            && !is_generic_title(s)
            && !is_noise_subject(s)
            && s.len() >= MIN_MEANINGFUL_CHARS
    })
}

/// Propose a better PR title when high-confidence signal exists.
pub fn propose_pr_title(
    current: &str,
    commits: &[String],
    changed_paths: &[String],
    guidelines_want_conventional: bool,
) -> Option<ProposedTitle> {
    if !title_needs_fix(current, guidelines_want_conventional) {
        return None;
    }

    // Prefer an existing conventional commit subject (strongest signal).
    if let Some(subj) = first_conventional_commit(commits) {
        let title = polish_subject(subj);
        if !is_conventional_subject(&title) {
            return None;
        }
        if title.eq_ignore_ascii_case(current.trim()) {
            return None;
        }
        return Some(ProposedTitle {
            title,
            auto_safe: true,
        });
    }

    if guidelines_want_conventional {
        let body_src = first_meaningful_commit(commits)
            .map(|s| s.to_string())
            .filter(|s| !is_conventional_subject(s))
            .or_else(|| {
                let t = current.trim();
                if !t.is_empty() && !is_generic_title(t) && !is_noise_subject(t) {
                    Some(t.to_string())
                } else {
                    None
                }
            });
        let body_raw = body_src?;
        let body = clean_body(&body_raw);
        if body.chars().count() < MIN_MEANINGFUL_CHARS {
            return None;
        }
        let ty = infer_type_from_paths(changed_paths);
        let title = finalize_title(&format!("{ty}: {body}"));
        if !is_conventional_subject(&title) {
            return None;
        }
        if title.eq_ignore_ascii_case(current.trim()) {
            return None;
        }
        return Some(ProposedTitle {
            title,
            auto_safe: true,
        });
    }

    // No conventional requirement: suggest a polished meaningful commit subject.
    if let Some(subj) = first_meaningful_commit(commits) {
        let title = polish_subject(subj);
        if title.chars().count() < MIN_MEANINGFUL_CHARS {
            return None;
        }
        if title.eq_ignore_ascii_case(current.trim()) {
            return None;
        }
        let auto_safe = is_conventional_subject(&title);
        return Some(ProposedTitle { title, auto_safe });
    }

    None
}

/// Resolve mode: repo flag wins when set in config_json; otherwise global setting.
pub fn resolve_mode(
    repo_mode: crate::config::PrTitleFixMode,
    global_raw: Option<&str>,
    repo_config_had_key: bool,
) -> crate::config::PrTitleFixMode {
    if repo_config_had_key {
        return repo_mode;
    }
    global_raw
        .map(crate::config::PrTitleFixMode::parse)
        .unwrap_or(repo_mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::rules::ExtractedRule;

    fn gf_conventional() -> GuidelineFile {
        let mut gf = GuidelineFile::from_content(
            "CONTRIBUTING.md",
            "CONTRIBUTING.md",
            "# Contributing\n".into(),
        )
        .unwrap();
        gf.rules = vec![ExtractedRule::CommitRule {
            description: "Use conventional commits type(scope): description".into(),
        }];
        gf
    }

    #[test]
    fn skips_when_already_good() {
        assert!(propose_pr_title(
            "feat(api): add retries",
            &["feat(api): add retries".into()],
            &["src/api.rs".into()],
            true,
        )
        .is_none());
    }

    #[test]
    fn reuses_conventional_commit() {
        let p = propose_pr_title(
            "update",
            &["feat(bot): add title fix".into(), "wip".into()],
            &["src/bot/title_fix.rs".into()],
            true,
        )
        .expect("proposal");
        assert_eq!(p.title, "feat(bot): add title fix");
        assert!(p.auto_safe);
    }

    #[test]
    fn polishes_imperative_and_lowercase() {
        let p = propose_pr_title(
            "wip",
            &["feat(api): Added Retries for webhooks.".into()],
            &["src/api.rs".into()],
            true,
        )
        .expect("proposal");
        assert_eq!(p.title, "feat(api): add retries for webhooks");
        assert!(!p.title.ends_with('.'));
        assert!(p.auto_safe);
    }

    #[test]
    fn prefixes_type_when_guidelines_require() {
        let p = propose_pr_title(
            "Add title autofix helper",
            &["Add title autofix helper".into()],
            &["src/bot/title_fix.rs".into()],
            true,
        )
        .expect("proposal");
        assert_eq!(p.title, "feat: add title autofix helper");
        assert!(p.auto_safe);
        assert!(is_conventional_subject(&p.title));
    }

    #[test]
    fn docs_paths_infer_docs_type() {
        let p = propose_pr_title(
            "Update docs",
            &["Clarify configuration for title fix".into()],
            &["docs/configuration.md".into()],
            true,
        )
        .expect("proposal");
        assert_eq!(p.title, "docs: clarify configuration for title fix");
    }

    #[test]
    fn soft_caps_long_titles() {
        let long = format!("feat: {}", "word ".repeat(40).trim());
        let p = propose_pr_title("update", &[long], &["src/a.rs".into()], true).expect("proposal");
        assert!(p.title.chars().count() <= SOFT_TITLE_CHARS);
        assert!(p.title.starts_with("feat: "));
    }

    #[test]
    fn skips_merge_noise() {
        assert!(propose_pr_title(
            "wip",
            &["Merge branch 'main' into feature".into()],
            &["src/a.rs".into()],
            true,
        )
        .is_none());
    }

    #[test]
    fn no_invent_without_signal() {
        assert!(propose_pr_title("wip", &[], &[], true).is_none());
        assert!(propose_pr_title("update", &["fix".into()], &[], false).is_none());
    }

    #[test]
    fn guidelines_want_detects_rule() {
        assert!(guidelines_want_conventional(&[gf_conventional()]));
        assert!(!guidelines_want_conventional(&[]));
    }

    #[test]
    fn resolve_mode_repo_wins() {
        use crate::config::PrTitleFixMode;
        assert_eq!(
            resolve_mode(PrTitleFixMode::Auto, Some("suggest"), true),
            PrTitleFixMode::Auto
        );
        assert_eq!(
            resolve_mode(PrTitleFixMode::Off, Some("suggest"), false),
            PrTitleFixMode::Suggest
        );
    }
}
