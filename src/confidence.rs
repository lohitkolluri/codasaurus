//! Confidence scoring for findings (0-5).
//!
//! Base confidence comes from detector class: deterministic registry checks
//! are certain, heuristic detectors need an LLM judge to be trusted.

use crate::detectors::Finding;

/// Confidence for a detector class, before any LLM judge runs.
///
/// - 5: registry / manifest ground truth (imports, deps, licenses, secrets, IaC)
/// - 3: heuristic detectors (style, slop, stale APIs, guidelines, graph)
/// - 3: vulnerabilities (manifest-only; reachable imports are set to 5 by the
///   vulnerabilities detector)
/// - 4: everything else (LLM-authored prose findings)
pub fn base_confidence(detector: &str) -> u8 {
    match detector {
        "hallucinated-imports"
        | "phantom-deps"
        | "lockfile-drift"
        | "license-drift"
        | "secrets"
        | "iac"
        | "risky-patterns" => 5,
        "vulnerabilities" => 3,
        "boilerplate" | "over-engineering" | "slop-detection" | "stale-api" | "graph"
        | "guidelines" | "todo-leaks" | "policy" => 3,
        _ => 4,
    }
}

/// Set `confidence` on any finding that does not already carry one.
pub fn apply_base(findings: &mut [Finding]) {
    for f in findings.iter_mut() {
        if f.confidence.is_none() {
            f.confidence = Some(base_confidence(&f.detector));
        }
    }
}

/// Drop findings the pipeline cannot ground: confidence <= 1.
pub fn retain_grounded(findings: &mut Vec<Finding>) {
    findings.retain(|f| f.confidence.unwrap_or(0) >= 2);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(detector: &str) -> Finding {
        Finding {
            detector: detector.to_string(),
            severity: "warning",
            file: "a.rs".into(),
            line: 1,
            column: 0,
            message: "m".into(),
            suggestion: None,
            evidence: None,
            codemod: None,
            confidence: None,
            judge_rationale: None,
            reachability: None,
        }
    }

    #[test]
    fn registry_detectors_are_max_confidence() {
        for d in [
            "hallucinated-imports",
            "phantom-deps",
            "lockfile-drift",
            "license-drift",
            "secrets",
            "iac",
            "risky-patterns",
        ] {
            assert_eq!(base_confidence(d), 5, "{d}");
        }
    }

    #[test]
    fn vulnerabilities_and_heuristics_are_3() {
        assert_eq!(base_confidence("vulnerabilities"), 3);
        for d in ["boilerplate", "stale-api", "slop-detection", "graph"] {
            assert_eq!(base_confidence(d), 3, "{d}");
        }
    }

    #[test]
    fn unknown_detector_defaults_to_4() {
        assert_eq!(base_confidence("some-llm-prose"), 4);
    }

    #[test]
    fn apply_base_fills_missing_only() {
        let mut fs = vec![finding("secrets"), finding("stale-api")];
        fs[1].confidence = Some(2);
        apply_base(&mut fs);
        assert_eq!(fs[0].confidence, Some(5));
        assert_eq!(fs[1].confidence, Some(2));
    }

    #[test]
    fn retain_grounded_drops_low_confidence() {
        let mut fs = vec![finding("x"), finding("y")];
        fs[0].confidence = Some(1);
        fs[1].confidence = Some(3);
        retain_grounded(&mut fs);
        assert_eq!(fs.len(), 1);
        assert_eq!(fs[0].confidence, Some(3));
    }
}
