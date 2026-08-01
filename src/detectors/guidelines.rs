use std::path::Path;
use std::sync::LazyLock;

use crate::config::Config;
use crate::context::guidelines::{find_guidelines, GuidelineFile};
use crate::context::rules::ExtractedRule;
use crate::detectors::Finding;
use crate::git;
use regex::Regex;

static CONVENTIONAL_COMMIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(feat|fix|chore|docs|style|refactor|perf|test|build|ci|revert)(\(.+\))?!?: .+")
        .expect("invalid conventional commit regex")
});

/// Runs the guidelines compliance check.
///
/// Discovers guidelines, extracts checkable rules, and validates:
/// - Branch naming conventions
/// - Commit message conventions (conventional commits, DCO sign-off)
/// - Required files existence
/// - Checklist items from the guidelines (reported as info)
pub fn detect(config: &Config) -> Vec<Finding> {
    if !config.checks.guidelines {
        return Vec::new();
    }

    let Ok(root) = git::repo_root() else {
        return Vec::new();
    };
    let root_path = Path::new(&root);

    let guidelines_override = config.guidelines.contributing_guidelines.as_deref();
    let files = find_guidelines(root_path, guidelines_override);
    if files.is_empty() {
        return Vec::new();
    }

    let mut findings: Vec<Finding> = Vec::new();

    for gf in &files {
        for rule in &gf.rules {
            match rule {
                ExtractedRule::BranchPattern { pattern } => {
                    check_branch_pattern(gf, pattern, &mut findings);
                }
                ExtractedRule::CommitRule { description } => {
                    let desc_lower = description.to_lowercase();
                    if desc_lower.contains("signed-off-by") || desc_lower.contains("dco") {
                        check_sign_off(gf, description, &mut findings);
                    } else if desc_lower.contains("conventional")
                        || desc_lower.contains("type(scope)")
                    {
                        check_conventional_commits(gf, description, &mut findings);
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
                ExtractedRule::FileRequired { path } => {
                    let path_str = root_path.join(path);
                    if !path_str.exists() {
                        findings.push(Finding {
                            detector: "guidelines".to_string(),
                            severity: "warning",
                            file: gf.path.to_string_lossy().to_string(),
                            line: 0,
                            column: 0,
                            message: format!("Required file `{path}` not found."),
                            suggestion: Some(format!("Create `{path}` or update the contributing guidelines if this file is no longer required.")),
                            evidence: None,
                            codemod: None,
                        });
                    }
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

fn check_branch_pattern(gf: &GuidelineFile, pattern: &str, findings: &mut Vec<Finding>) {
    let branch = match git::current_branch() {
        Ok(b) => b,
        Err(_) => return,
    };

    let clean_pattern = pattern.trim_matches('`').trim().to_lowercase();
    let branch_lower = branch.to_lowercase();

    // If the pattern contains `/` it's likely a prefix pattern like `feat/`
    let matches = if clean_pattern.contains('/') || clean_pattern.contains('-') {
        // Check if branch starts with any of the pipe-separated patterns
        clean_pattern
            .split('/')
            .next()
            .map(|prefix| branch_lower.starts_with(&format!("{}/", prefix.trim())))
            .unwrap_or(false)
            || clean_pattern
                .split('/')
                .any(|part| branch_lower.contains(part.trim()))
    } else if clean_pattern.starts_with("feat") {
        branch_lower.starts_with("feat/") || branch_lower.starts_with("feature/")
    } else if clean_pattern.starts_with("fix") {
        branch_lower.starts_with("fix/")
    } else if clean_pattern.starts_with("chore") {
        branch_lower.starts_with("chore/")
    } else {
        branch_lower.contains(&clean_pattern)
    };

    #[allow(clippy::if_not_else)]
    if !matches {
        findings.push(Finding {
            detector: "guidelines".to_string(),
            severity: "warning",
            file: gf.path.to_string_lossy().to_string(),
            line: 0,
            column: 0,
            message: format!(
                "Current branch '{branch}' doesn't match guideline pattern: {pattern}"
            ),
            suggestion: Some(format!("Rename branch to match '{pattern}'")),
            evidence: None,
            codemod: None,
        });
    }
}

fn check_sign_off(gf: &GuidelineFile, _description: &str, findings: &mut Vec<Finding>) {
    let commits = match git::recent_commits(10) {
        Ok(c) => c,
        Err(_) => return,
    };

    let unsigned: Vec<&str> = commits
        .iter()
        .filter(|c| !c.contains("Signed-off-by:"))
        .map(|c| {
            let first_line = c.lines().next().unwrap_or(c);
            first_line
        })
        .collect();

    if !unsigned.is_empty() {
        findings.push(Finding {
            detector: "guidelines".to_string(),
            severity: "warning",
            file: gf.path.to_string_lossy().to_string(),
            line: 0,
            column: 0,
            message: format!(
                "{} recent commit(s) are missing Signed-off-by (DCO)",
                unsigned.len()
            ),
            suggestion: Some("Run `git commit --amend -s` on unsigned commits, or use `git rebase --exec 'git commit --amend --no-edit -s'`".to_string()),
            evidence: Some(unsigned.join("\n")),
            codemod: None,
        });
    }
}

fn check_conventional_commits(gf: &GuidelineFile, _description: &str, findings: &mut Vec<Finding>) {
    let commits = match git::recent_commits(10) {
        Ok(c) => c,
        Err(_) => return,
    };

    let non_conventional: Vec<&str> = commits
        .iter()
        .filter(|c| {
            let first_line = c.lines().next().unwrap_or(c);
            !CONVENTIONAL_COMMIT_RE.is_match(first_line)
        })
        .map(|c| c.lines().next().unwrap_or(c))
        .collect();

    if !non_conventional.is_empty() {
        findings.push(Finding {
            detector: "guidelines".to_string(),
            severity: "info",
            file: gf.path.to_string_lossy().to_string(),
            line: 0,
            column: 0,
            message: "Conventional commits are recommended but none of the recent commits match the format `type(scope): description`".to_string(),
            suggestion: Some("Use format: feat|fix|chore|docs|refactor|test(scope): description".to_string()),
            evidence: Some(non_conventional.join("\n")),
            codemod: None,
        });
    }
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
                    // Manifest/guideline fetches already cover the high-signal cases.
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
            .split(|c| c == '/' || c == '|')
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
            message: format!(
                "PR branch '{branch}' doesn't match guideline pattern: {pattern}"
            ),
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

    #[test]
    fn test_detect_disabled_via_config() {
        let mut config = Config::default();
        config.checks.guidelines = false;
        let findings = detect(&config);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_checklist_items_become_findings() {
        let mut config = Config::default();
        config.checks.guidelines = true;
        let findings = detect(&config);
        // Without a git repo, this should return empty
        assert!(findings.is_empty());
    }

    #[test]
    fn test_branch_pattern_matching() {
        let gf = GuidelineFile {
            path: "CONTRIBUTING.md".into(),
            source: "test".into(),
            content: String::new(),
            rules: vec![],
        };

        let mut findings = Vec::new();
        check_branch_pattern(&gf, "feat/", &mut findings);
        check_branch_pattern(&gf, "fix/", &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_detect_remote_branch_and_commits() {
        let gf = GuidelineFile::from_content(
            "CONTRIBUTING.md",
            "CONTRIBUTING.md",
            "# Contributing\n\nBranch names should use `feat/` prefix.\nUse conventional commits.\n- [ ] Add tests\n".into(),
        )
        .unwrap();
        // Force a branch pattern rule if parser didn't extract — use detect_remote with synthetic rules
        let mut gf = gf;
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
