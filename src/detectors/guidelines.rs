//! Contribution guidelines checks for the PR bot (remote content only).
//!
//! Local git-repo scanning was retired with the CLI. Use [`detect_remote`] with
//! files fetched via the GitHub Contents API.

use std::sync::LazyLock;

use crate::context::guidelines::GuidelineFile;
use crate::context::rules::ExtractedRule;
use crate::detectors::Finding;
use regex::Regex;

static CONVENTIONAL_COMMIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(feat|fix|chore|docs|style|refactor|perf|test|build|ci|revert)(\(.+\))?!?: .+")
        .expect("invalid conventional commit regex")
});

/// Deprecated local-FS path — always empty. Bot uses [`detect_remote`].
pub fn detect(_config: &crate::config::Config) -> Vec<Finding> {
    Vec::new()
}

/// Runs guidelines checks against in-memory guideline files (bot / remote path).
///
/// Uses PR branch + commit messages instead of local `git`.
pub fn detect_remote(
    files: &[GuidelineFile],
    branch: &str,
    commit_messages: &[String],
    changed_paths: &[String],
) -> Vec<Finding> {
    if files.is_empty() {
        return Vec::new();
    }

    let mut findings: Vec<Finding> = Vec::new();
    let _ = changed_paths;

    for gf in files {
        for rule in &gf.rules {
            match rule {
                ExtractedRule::BranchPattern { pattern } => {
                    check_branch_pattern_remote(gf, pattern, branch, &mut findings);
                }
                ExtractedRule::CommitRule { description } => {
                    let desc_lower = description.to_lowercase();
                    if desc_lower.contains("signed-off-by") || desc_lower.contains("dco") {
                        check_sign_off_remote(gf, commit_messages, &mut findings);
                    } else if desc_lower.contains("conventional")
                        || desc_lower.contains("type(scope)")
                    {
                        check_conventional_remote(gf, commit_messages, &mut findings);
                    }
                }
                ExtractedRule::ChecklistItem { text, .. } => {
                    findings.push(Finding {
                        detector: "guidelines".to_string(),
                        severity: "info",
                        file: gf.path.to_string_lossy().to_string(),
                        line: 0,
                        column: 0,
                        message: format!("Guideline checklist item: {text}"),
                        suggestion: None,
                        evidence: None,
                        codemod: None,
                    });
                }
                ExtractedRule::FileRequired { path: _ } => {
                    // Skip: verifying required files needs extra Contents GETs per path.
                }
                ExtractedRule::SectionRule { heading, text } => {
                    findings.push(Finding {
                        detector: "guidelines".to_string(),
                        severity: "info",
                        file: gf.path.to_string_lossy().to_string(),
                        line: 0,
                        column: 0,
                        message: format!(
                            "Guideline section: {} — {}",
                            heading,
                            text.chars().take(200).collect::<String>()
                        ),
                        suggestion: None,
                        evidence: None,
                        codemod: None,
                    });
                }
            }
        }
    }

    findings
}

fn check_branch_pattern_remote(
    gf: &GuidelineFile,
    pattern: &str,
    branch: &str,
    findings: &mut Vec<Finding>,
) {
    if branch.is_empty() {
        return;
    }
    let clean_pattern = pattern.trim_matches('`').trim().to_lowercase();
    let branch_lower = branch.to_lowercase();

    let matches = if clean_pattern.contains('/') || clean_pattern.contains('-') {
        let parts: Vec<&str> = clean_pattern
            .split(['/', '|'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        parts
            .first()
            .map(|prefix| branch_lower.starts_with(&format!("{prefix}/")))
            .unwrap_or(false)
            || parts.iter().any(|part| branch_lower.contains(part))
    } else if clean_pattern.starts_with("feat") {
        branch_lower.starts_with("feat/") || branch_lower.starts_with("feature/")
    } else if clean_pattern.starts_with("fix") {
        branch_lower.starts_with("fix/")
    } else if clean_pattern.starts_with("chore") {
        branch_lower.starts_with("chore/")
    } else {
        branch_lower.contains(&clean_pattern)
    };

    if !matches {
        findings.push(Finding {
            detector: "guidelines".to_string(),
            severity: "warning",
            file: gf.path.to_string_lossy().to_string(),
            line: 0,
            column: 0,
            message: format!("PR branch '{branch}' doesn't match guideline pattern: {pattern}"),
            suggestion: Some(format!("Rename branch to match '{pattern}'")),
            evidence: None,
            codemod: None,
        });
    }
}

fn check_sign_off_remote(
    gf: &GuidelineFile,
    commit_messages: &[String],
    findings: &mut Vec<Finding>,
) {
    if commit_messages.is_empty() {
        return;
    }
    let unsigned: Vec<&str> = commit_messages
        .iter()
        .filter(|c| !c.contains("Signed-off-by:"))
        .map(|c| c.lines().next().unwrap_or(c))
        .collect();

    if !unsigned.is_empty() {
        findings.push(Finding {
            detector: "guidelines".to_string(),
            severity: "warning",
            file: gf.path.to_string_lossy().to_string(),
            line: 0,
            column: 0,
            message: format!(
                "{} PR commit(s) are missing Signed-off-by (DCO)",
                unsigned.len()
            ),
            suggestion: Some(
                "Amend commits with `-s` / Signed-off-by as required by contributing guidelines."
                    .into(),
            ),
            evidence: Some(unsigned.join("\n")),
            codemod: None,
        });
    }
}

fn check_conventional_remote(
    gf: &GuidelineFile,
    commit_messages: &[String],
    findings: &mut Vec<Finding>,
) {
    if commit_messages.is_empty() {
        return;
    }
    let non_conventional: Vec<&str> = commit_messages
        .iter()
        .filter(|c| {
            let first_line = c.lines().next().unwrap_or(c);
            !CONVENTIONAL_COMMIT_RE.is_match(first_line)
        })
        .map(|c| c.lines().next().unwrap_or(c.as_str()))
        .collect();

    if !non_conventional.is_empty() {
        findings.push(Finding {
            detector: "guidelines".to_string(),
            severity: "info",
            file: gf.path.to_string_lossy().to_string(),
            line: 0,
            column: 0,
            message: "Conventional commits are recommended but some PR commits don't match `type(scope): description`".into(),
            suggestion: Some(
                "Use format: feat|fix|chore|docs|refactor|test(scope): description".into(),
            ),
            evidence: Some(non_conventional.join("\n")),
            codemod: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_detect_local_is_noop() {
        let mut config = Config::default();
        config.checks.guidelines = true;
        assert!(detect(&config).is_empty());
    }

    #[test]
    fn test_detect_remote_branch_and_commits() {
        let mut gf = GuidelineFile::from_content(
            "CONTRIBUTING.md",
            "CONTRIBUTING.md",
            "# Contributing\n".into(),
        )
        .unwrap();
        gf.rules = vec![
            ExtractedRule::BranchPattern {
                pattern: "feat/".into(),
            },
            ExtractedRule::ChecklistItem {
                text: "Add tests".into(),
                checked: false,
            },
        ];
        let findings = detect_remote(
            &[gf],
            "fix/typo",
            &["wip stuff".into()],
            &["src/a.rs".into()],
        );
        assert!(findings.iter().any(|f| f.message.contains("doesn't match")));
        assert!(findings.iter().any(|f| f.message.contains("checklist")));
    }
}
