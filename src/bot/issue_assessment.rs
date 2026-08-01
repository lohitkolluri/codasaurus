//! Heuristic linked-issue / ticket assessment for walkthroughs.

use crate::llm::IssueContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueVerdict {
    Addressed,
    Partial,
    Unclear,
}

impl IssueVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Addressed => "addressed",
            Self::Partial => "partial",
            Self::Unclear => "unclear",
        }
    }
}

/// Score whether PR title + changed paths appear to address linked tickets.
pub fn assess_linked_issues(
    pr_title: &str,
    changed_paths: &[String],
    issues: &[IssueContext],
) -> Vec<(IssueContext, IssueVerdict)> {
    issues
        .iter()
        .cloned()
        .map(|issue| {
            let verdict = score_issue(pr_title, changed_paths, &issue);
            (issue, verdict)
        })
        .collect()
}

fn score_issue(pr_title: &str, paths: &[String], issue: &IssueContext) -> IssueVerdict {
    let title_tokens = significant_tokens(&issue.title);
    if title_tokens.is_empty() {
        return IssueVerdict::Unclear;
    }
    let haystack = format!(
        "{} {}",
        pr_title.to_ascii_lowercase(),
        paths.join(" ").to_ascii_lowercase()
    );
    let hits = title_tokens
        .iter()
        .filter(|t| haystack.contains(t.as_str()))
        .count();
    let ratio = hits as f32 / title_tokens.len() as f32;
    if ratio >= 0.45 {
        IssueVerdict::Addressed
    } else if ratio >= 0.2 || hits >= 1 {
        IssueVerdict::Partial
    } else {
        IssueVerdict::Unclear
    }
}

fn significant_tokens(s: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "a", "an", "and", "or", "to", "for", "of", "in", "on", "with", "from", "by", "is",
        "are", "be", "this", "that", "fix", "add", "update", "implement", "support", "issue",
        "jira", "linear",
    ];
    s.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .map(|t| t.to_ascii_lowercase())
        .filter(|t| t.len() >= 3 && !STOP.contains(&t.as_str()))
        .take(12)
        .collect()
}

/// Markdown block for walkthrough.
pub fn assessment_markdown(rows: &[(IssueContext, IssueVerdict)]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::from("#### Linked issues\n\n");
    out.push_str("| Ticket | Assessment |\n| --- | --- |\n");
    for (issue, verdict) in rows {
        let label = if issue.number > 0 {
            format!("#{} — {}", issue.number, issue.title)
        } else {
            issue.title.clone()
        };
        let chip = format!("`{}`", verdict.as_str());
        out.push_str(&format!(
            "| {} | {chip} |\n",
            label.replace('|', "\\|"),
        ));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_path_overlap_as_partial_or_addressed() {
        let issue = IssueContext {
            number: 12,
            title: "Fix auth middleware timeout".into(),
            body: None,
        };
        let v = score_issue(
            "harden auth middleware",
            &["src/auth/middleware.rs".into()],
            &issue,
        );
        assert!(matches!(v, IssueVerdict::Addressed | IssueVerdict::Partial));
    }

    #[test]
    fn unclear_when_unrelated() {
        let issue = IssueContext {
            number: 1,
            title: "Redesign billing invoices".into(),
            body: None,
        };
        let v = score_issue("typo in readme", &["README.md".into()], &issue);
        assert_eq!(v, IssueVerdict::Unclear);
    }
}
