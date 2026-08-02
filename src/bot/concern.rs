//! Map detectors (and paths) onto senior-reviewer concerns.

/// Specialist concern buckets used in overview / Stats (not multi-agent infra).
pub fn concern_for_detector(detector: &str) -> &'static str {
    match detector {
        "secrets" | "vulnerabilities" | "iac" | "risky-patterns" | "risky_patterns" => "security",
        "guidelines" => "docs",
        "slop" | "slop-detection" => "quality",
        _ => "quality",
    }
}

/// Prefer path signal for tests/docs when the detector is generic quality.
pub fn concern_for_finding(detector: &str, file: &str) -> &'static str {
    let base = concern_for_detector(detector);
    let l = file.to_ascii_lowercase();
    if base == "quality" {
        if l.contains("/test/")
            || l.starts_with("tests/")
            || l.starts_with("test/")
            || l.contains("/tests/")
            || l.ends_with("_test.rs")
            || l.ends_with(".test.ts")
            || l.ends_with(".spec.ts")
            || l.ends_with("_test.go")
            || l.ends_with("_test.py")
        {
            return "tests";
        }
        if l.starts_with("docs/")
            || l.contains("/docs/")
            || l.ends_with(".md")
            || l == "readme.md"
            || l == "changelog.md"
        {
            return "docs";
        }
    }
    base
}

/// Tier-1 findings that should be able to drive REQUEST_CHANGES / hard merge posture.
pub fn is_hard_tier1(detector: &str, severity: &str) -> bool {
    if severity == "blocking" {
        return true;
    }
    severity == "warning" && concern_for_detector(detector) == "security"
}

/// Review confidence from Tier-1 findings alone (LLM posts separately).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewConfidence {
    High,
    Medium,
    Low,
}

pub fn overall_confidence(
    findings: &[crate::detectors::Finding],
    has_blocking: bool,
) -> ReviewConfidence {
    if has_blocking {
        return ReviewConfidence::High;
    }
    let hard = findings
        .iter()
        .filter(|f| is_hard_tier1(&f.detector, f.severity))
        .count();
    if hard > 0 {
        return ReviewConfidence::High;
    }
    if findings.is_empty() {
        return ReviewConfidence::High;
    }
    let soft_only = findings.iter().all(|f| {
        matches!(
            f.detector.as_str(),
            "slop"
                | "slop-detection"
                | "guidelines"
                | "boilerplate"
                | "over-engineering"
                | "todo-leaks"
        ) || f.severity == "info"
    });
    if soft_only {
        ReviewConfidence::Low
    } else {
        ReviewConfidence::Medium
    }
}

/// GitHub pull-request review event.
/// REQUEST_CHANGES only when Tier-1 blocking exists. Opt-in APPROVE when clean.
pub fn review_event(
    has_tier1_blocking: bool,
    findings_empty: bool,
    auto_approve: bool,
) -> &'static str {
    if has_tier1_blocking {
        "REQUEST_CHANGES"
    } else if findings_empty && auto_approve {
        "APPROVE"
    } else {
        "COMMENT"
    }
}

/// Soft findings only → advisory draft overview (human still merges).
pub fn is_advisory_draft(findings: &[crate::detectors::Finding], has_blocking: bool) -> bool {
    if has_blocking {
        return false;
    }
    if findings.is_empty() {
        return false;
    }
    matches!(
        overall_confidence(findings, has_blocking),
        ReviewConfidence::Low
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::Finding;

    fn f(detector: &str, severity: &'static str, file: &str) -> Finding {
        Finding {
            detector: detector.into(),
            severity,
            file: file.into(),
            line: 1,
            column: 0,
            message: "x".into(),
            suggestion: None,
            evidence: None,
            codemod: None,
        }
    }

    #[test]
    fn maps_security_and_tests() {
        assert_eq!(concern_for_finding("secrets", "src/a.rs"), "security");
        assert_eq!(concern_for_finding("boilerplate", "tests/a.rs"), "tests");
        assert_eq!(concern_for_finding("boilerplate", "docs/x.md"), "docs");
        assert_eq!(concern_for_finding("stale-api", "src/a.rs"), "quality");
    }

    #[test]
    fn review_event_policy() {
        assert_eq!(review_event(true, false, true), "REQUEST_CHANGES");
        assert_eq!(review_event(false, true, true), "APPROVE");
        assert_eq!(review_event(false, true, false), "COMMENT");
        assert_eq!(review_event(false, false, true), "COMMENT");
    }

    #[test]
    fn soft_findings_are_advisory() {
        let soft = vec![f("slop-detection", "info", "PR")];
        assert!(is_advisory_draft(&soft, false));
        assert_eq!(overall_confidence(&soft, false), ReviewConfidence::Low);
        let hard = vec![f("secrets", "warning", "src/a.rs")];
        assert!(!is_advisory_draft(&hard, false));
    }
}
