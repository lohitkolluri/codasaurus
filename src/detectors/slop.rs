//! Detects AI-generated/slop PRs using lightweight heuristics.
//! No LLM calls — pure pattern matching on PR metadata and content.

use crate::detectors::Finding;
use crate::parser::ParsedFile;
use std::sync::LazyLock;

static GENERIC_TITLE_WORDS: LazyLock<aho_corasick::AhoCorasick> = LazyLock::new(|| {
    aho_corasick::AhoCorasick::new([
        "fix some issues",
        "various changes",
        "improve code",
        "update",
        "changes",
        "fix",
        "wip",
    ])
    .expect("valid Aho-Corasick patterns")
});

/// Analyze file content for AI-generation signals.
/// Returns findings if the PR appears AI-generated.
pub fn detect_slop(
    files: &[ParsedFile],
    pr_title: &str,
    pr_body: &str,
    commit_messages: &[String],
) -> Vec<Finding> {
    let mut score = 0u8;
    let mut reasons: Vec<&str> = Vec::new();

    // Check 1: Vague or empty PR title
    let title_lower = pr_title.to_lowercase();
    if pr_title.len() < 10 {
        score += 2;
        reasons.push("Very short or missing PR title");
    } else if GENERIC_TITLE_WORDS.is_match(&title_lower) {
        score += 3;
        reasons.push("Generic PR title typical of AI output");
    }

    // Check 2: Empty or very short PR description
    let body_trimmed = pr_body.trim();
    if body_trimmed.is_empty() || body_trimmed.len() < 30 {
        score += 3;
        reasons.push("Missing or minimal PR description");
    }

    // Check 3: Many files changed (bulk AI changes)
    if files.len() > 30 {
        score += 2;
        reasons.push("Large number of files changed in single PR");
    } else if files.len() > 15 {
        score += 1;
        reasons.push("Moderate number of files changed");
    }

    // Check 4: All commit messages are identical
    if commit_messages.len() > 1 {
        let first = &commit_messages[0];
        if commit_messages.iter().all(|m| m == first) {
            score += 2;
            reasons.push("All commit messages are identical");
        }
    }

    // Check 5: High comment-to-code ratio
    let total_lines: usize = files.iter().map(|f| f.lines.len()).sum();
    let comment_lines: usize = files
        .iter()
        .map(|f| {
            f.lines
                .iter()
                .filter(|l| {
                    let t = l.content.trim();
                    t.starts_with("//")
                        || t.starts_with('#')
                        || t.starts_with("/*")
                        || t.starts_with('*')
                })
                .count()
        })
        .sum();
    if total_lines > 0 && comment_lines > total_lines / 3 {
        score += 1;
        reasons.push("High comment-to-code ratio");
    }

    if score >= 4 {
        let path = files.first().map(|f| f.path.as_str()).unwrap_or("PR");
        vec![Finding {
            detector: "slop-detection".to_string(),
            severity: if score >= 7 { "warning" } else { "info" },
            file: path.to_string(),
            line: 0,
            column: 0,
            message: format!(
                "Possible AI-generated PR (score: {}/10). {}",
                score,
                reasons.join("; ")
            ),
            suggestion: Some(
                "Review changes carefully — AI-generated code may contain hallucinated \
                 imports, phantom dependencies, and subtle logic errors. Consider splitting \
                 into smaller PRs for easier review."
                    .to_string(),
            ),
            evidence: None,
            codemod: None,
            confidence: None,
            judge_rationale: None,
        }]
    } else {
        vec![]
    }
}
