//! Finding provenance lines for Tier-1 auditability + LLM confidence gates.

use crate::detectors::Finding;

/// Short provenance footer for inline comments / walkthrough rows.
pub fn provenance_line(f: &Finding) -> String {
    let mut parts = vec![
        format!("**detector:** `{}`", f.detector),
        "**source:** `tier1`".into(),
    ];
    if let Some(url) = evidence_url(f) {
        parts.push(format!("**evidence:** {url}"));
    }
    if let Some(ev) = f.evidence.as_ref().filter(|e| !e.is_empty()) {
        let snip: String = ev.chars().take(80).collect();
        parts.push(format!("**snippet:** `{}`", snip.replace('`', "'")));
    }
    parts.join("  \n")
}

fn evidence_url(f: &Finding) -> Option<String> {
    match f.detector.as_str() {
        "hallucinated-imports" | "phantom-deps" => {
            let pkg = f.message.split('`').nth(1)?;
            let enc = urlencoding_lite(pkg);
            let lower = f.file.to_lowercase();
            let url = if lower.ends_with(".rs") || lower.contains("cargo.toml") {
                format!("https://crates.io/search?q={enc}")
            } else if lower.ends_with(".py")
                || lower.contains("requirements")
                || lower.contains("pyproject.toml")
            {
                format!("https://pypi.org/search/?q={enc}")
            } else if lower.ends_with(".go") || lower.contains("go.mod") {
                format!("https://pkg.go.dev/search?q={enc}")
            } else {
                format!("https://www.npmjs.com/search?q={enc}")
            };
            Some(url)
        }
        "vulnerabilities" => {
            let cve = f
                .message
                .split_whitespace()
                .find(|w| w.starts_with("CVE-") || w.starts_with("GHSA-"))?;
            Some(format!("https://osv.dev/vulnerability/{cve}"))
        }
        "iac" => Some("https://www.cisecurity.org/benchmark/kubernetes".into()),
        _ => None,
    }
}

fn urlencoding_lite(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

/// True when `candidate` matches a known PR path with path-component boundaries
/// (avoids `app.ts` matching `not-app.ts`).
fn path_matches(known: &str, candidate: &str) -> bool {
    if known == candidate {
        return true;
    }
    known.ends_with(&format!("/{candidate}")) || candidate.ends_with(&format!("/{known}"))
}

fn confidence_rank(conf: &str) -> u8 {
    match conf.to_ascii_lowercase().as_str() {
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

/// Drop LLM issues that fail path/evidence/confidence gates (hallucination + HITL posture).
///
/// Rules (Antern-style confidence gate, auto-post path):
/// - unknown path → drop
/// - `confidence=low` or missing → drop (never auto-post unsure findings)
/// - empty/short description (no rationale) → drop
/// - non-info without a line citation → drop
/// - `critical` + only `medium` confidence → downgrade severity to `warning`
pub fn reverify_llm_issues(
    issues: &[crate::llm::LlmIssue],
    known_paths: &[String],
    file_contents: &[(String, String)],
) -> Vec<crate::llm::LlmIssue> {
    issues
        .iter()
        .filter_map(|issue| {
            if issue.file.is_empty() || issue.file == "?" {
                return None;
            }
            let path_ok = known_paths.iter().any(|p| path_matches(p, &issue.file));
            if !path_ok {
                return None;
            }

            let conf = confidence_rank(&issue.confidence);
            if conf < 2 {
                tracing::debug!(
                    file = %issue.file,
                    confidence = %issue.confidence,
                    "dropping low-confidence LLM issue"
                );
                return None;
            }

            let rationale = issue
                .rationale
                .as_deref()
                .unwrap_or(issue.description.as_str())
                .trim();
            if rationale.chars().count() < 16 {
                return None;
            }

            let sev = issue.severity.to_ascii_lowercase();
            if issue.line == 0 && sev != "info" {
                return None;
            }

            // Weak content check when patch is available.
            if let Some((_, content)) = file_contents
                .iter()
                .find(|(p, _)| path_matches(p, &issue.file))
            {
                let needle = issue
                    .description
                    .split_whitespace()
                    .find(|w| w.len() >= 6)
                    .unwrap_or("");
                if !needle.is_empty()
                    && !content
                        .to_ascii_lowercase()
                        .contains(&needle.to_ascii_lowercase())
                    && issue.line == 0
                {
                    return None;
                }
            }

            let mut out = issue.clone();
            // Critical claims need high confidence to stay critical when auto-posted.
            if sev == "critical" && conf < 3 {
                out.severity = "warning".into();
            }
            Some(out)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmIssue;

    fn issue(file: &str, conf: &str, sev: &str, line: usize, desc: &str) -> LlmIssue {
        LlmIssue {
            severity: sev.into(),
            category: "logic".into(),
            file: file.into(),
            line,
            description: desc.into(),
            suggestion: None,
            confidence: conf.into(),
            rationale: None,
        }
    }

    #[test]
    fn provenance_includes_detector() {
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
        let line = provenance_line(&f);
        assert!(line.contains("secrets"));
        assert!(line.contains("tier1"));
    }

    #[test]
    fn drops_llm_issue_with_unknown_path() {
        let issues = vec![issue(
            "does/not/exist.rs",
            "medium",
            "warning",
            1,
            "something odd here definitely",
        )];
        let kept = reverify_llm_issues(&issues, &["src/main.rs".into()], &[]);
        assert!(kept.is_empty());
    }

    #[test]
    fn rejects_partial_path_suffix_match() {
        let issues = vec![issue(
            "app.ts",
            "medium",
            "warning",
            1,
            "something odd here definitely",
        )];
        let kept = reverify_llm_issues(&issues, &["src/not-app.ts".into()], &[]);
        assert!(kept.is_empty());
        let kept_ok = reverify_llm_issues(&issues, &["src/app.ts".into()], &[]);
        assert_eq!(kept_ok.len(), 1);
    }

    #[test]
    fn drops_low_confidence() {
        let issues = vec![issue(
            "src/a.rs",
            "low",
            "warning",
            3,
            "possible null dereference on user input",
        )];
        let kept = reverify_llm_issues(&issues, &["src/a.rs".into()], &[]);
        assert!(kept.is_empty());
    }

    #[test]
    fn downgrades_critical_without_high_confidence() {
        let issues = vec![issue(
            "src/a.rs",
            "medium",
            "critical",
            3,
            "SQL injection via string concat on query",
        )];
        let kept = reverify_llm_issues(&issues, &["src/a.rs".into()], &[]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].severity, "warning");
    }

    #[test]
    fn drops_short_rationale() {
        let issues = vec![issue("src/a.rs", "high", "warning", 1, "bad")];
        let kept = reverify_llm_issues(&issues, &["src/a.rs".into()], &[]);
        assert!(kept.is_empty());
    }
}
