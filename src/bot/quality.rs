//! High-signal filtering for org-scale PR reviews (severity budgets + noise rank).

use crate::detectors::Finding;

/// Cap how many findings of each severity are surfaced as inline comments / walkthrough rows.
#[derive(Debug, Clone)]
pub struct SignalBudget {
    pub max_blocking: usize,
    pub max_warning: usize,
    pub max_info: usize,
}

impl Default for SignalBudget {
    fn default() -> Self {
        Self {
            max_blocking: 20,
            max_warning: 8,
            max_info: 3,
        }
    }
}

/// Lower = higher priority for surfacing.
fn noise_rank(f: &Finding) -> u8 {
    let sev = match f.severity {
        "blocking" => 0,
        "warning" => 10,
        _ => 20,
    };
    let detector = match f.detector.as_str() {
        "secrets" => 0,
        "hallucinated-imports" => 1,
        "phantom-deps" => 2,
        "vulnerabilities" => 3,
        "todo-leaks" => 4,
        "stale-api" => 5,
        "guidelines" => 6,
        "slop" => 7,
        "over-engineering" | "boilerplate" => 8,
        "graph" => 9,
        _ => 10,
    };
    sev + detector
}

/// Keep high-signal findings only; preserves relative order within the same rank.
pub fn apply_signal_budget(findings: &mut Vec<Finding>, budget: &SignalBudget) {
    findings.sort_by_key(|f| (noise_rank(f), f.file.clone(), f.line));

    let mut blocking = 0usize;
    let mut warning = 0usize;
    let mut info = 0usize;
    findings.retain(|f| match f.severity {
        "blocking" => {
            blocking += 1;
            blocking <= budget.max_blocking
        }
        "warning" => {
            warning += 1;
            warning <= budget.max_warning
        }
        _ => {
            info += 1;
            info <= budget.max_info
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(det: &str, sev: &'static str, line: usize) -> Finding {
        Finding {
            detector: det.into(),
            severity: sev,
            file: "a.rs".into(),
            line,
            column: 0,
            message: format!("{det}-{line}"),
            suggestion: None,
            evidence: None,
            codemod: None,
        }
    }

    #[test]
    fn budget_keeps_secrets_over_graph_info() {
        let mut findings = vec![
            finding("graph", "info", 1),
            finding("graph", "info", 2),
            finding("graph", "info", 3),
            finding("graph", "info", 4),
            finding("secrets", "blocking", 5),
            finding("todo-leaks", "warning", 6),
            finding("todo-leaks", "warning", 7),
            finding("todo-leaks", "warning", 8),
            finding("todo-leaks", "warning", 9),
            finding("todo-leaks", "warning", 10),
            finding("boilerplate", "warning", 11),
            finding("boilerplate", "warning", 12),
            finding("boilerplate", "warning", 13),
            finding("boilerplate", "warning", 14),
            finding("boilerplate", "warning", 15),
        ];
        apply_signal_budget(
            &mut findings,
            &SignalBudget {
                max_blocking: 5,
                max_warning: 3,
                max_info: 1,
            },
        );
        assert!(findings.iter().any(|f| f.detector == "secrets"));
        assert_eq!(findings.iter().filter(|f| f.severity == "info").count(), 1);
        assert!(findings.iter().filter(|f| f.severity == "warning").count() <= 3);
    }
}
