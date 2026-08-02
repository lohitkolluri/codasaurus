use crate::config::QualityGateConfig;
use crate::detectors::Finding;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Sonar-style quality gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityGate {
    pub name: String,
    pub conditions: Vec<GateCondition>,
}

impl Default for QualityGate {
    fn default() -> Self {
        Self {
            name: "codasaurus way".into(),
            conditions: vec![
                GateCondition {
                    metric: GateMetric::NewBlockerIssues,
                    operator: GateOperator::Gt,
                    threshold: 0.0,
                },
                GateCondition {
                    metric: GateMetric::NewHighIssues,
                    operator: GateOperator::Gt,
                    threshold: 0.0,
                },
                GateCondition {
                    metric: GateMetric::NewMediumIssues,
                    operator: GateOperator::Gt,
                    threshold: 5.0,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateCondition {
    pub metric: GateMetric,
    pub operator: GateOperator,
    pub threshold: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateMetric {
    NewIssues,
    NewBlockerIssues,
    NewHighIssues,
    NewMediumIssues,
    NewWarningIssues,
    NewInfoIssues,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateOperator {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
    Ne,
}

impl GateOperator {
    fn eval(&self, value: f64, threshold: f64) -> bool {
        match self {
            GateOperator::Gt => value > threshold,
            GateOperator::Gte => value >= threshold,
            GateOperator::Lt => value < threshold,
            GateOperator::Lte => value <= threshold,
            GateOperator::Eq => (value - threshold).abs() < f64::EPSILON,
            GateOperator::Ne => (value - threshold).abs() >= f64::EPSILON,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    pub passed: bool,
    pub failed_conditions: Vec<FailedCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedCondition {
    pub metric: GateMetric,
    pub operator: GateOperator,
    pub threshold: f64,
    pub actual: f64,
}

/// Count new findings by severity.
fn severity_counts(findings: &[Finding]) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for f in findings {
        *counts.entry(f.severity).or_insert(0) += 1;
    }
    counts
}

/// Evaluate a gate against a set of findings.
///
/// All metrics operate on the provided findings (which are already filtered to
/// new-code lines by the baseline layer). Any failed condition causes the gate
/// to fail (OR semantics).
pub fn evaluate_gate(gate: &QualityGate, findings: &[Finding]) -> GateResult {
    let counts = severity_counts(findings);
    let mut failed = Vec::new();
    for cond in &gate.conditions {
        let actual = match cond.metric {
            GateMetric::NewIssues => findings.len() as f64,
            GateMetric::NewBlockerIssues => *counts.get("blocking").unwrap_or(&0) as f64,
            GateMetric::NewHighIssues => *counts.get("blocking").unwrap_or(&0) as f64,
            GateMetric::NewMediumIssues => *counts.get("warning").unwrap_or(&0) as f64,
            GateMetric::NewWarningIssues => *counts.get("warning").unwrap_or(&0) as f64,
            GateMetric::NewInfoIssues => *counts.get("info").unwrap_or(&0) as f64,
        };
        if cond.operator.eval(actual, cond.threshold) {
            failed.push(FailedCondition {
                metric: cond.metric,
                operator: cond.operator,
                threshold: cond.threshold,
                actual,
            });
        }
    }
    GateResult {
        passed: failed.is_empty(),
        failed_conditions: failed,
    }
}

/// Human-readable summary for a check run.
pub fn gate_summary(result: &GateResult) -> String {
    if result.passed {
        return "Quality gate passed".into();
    }
    let mut s = "Quality gate failed:\n".to_string();
    for f in &result.failed_conditions {
        s.push_str(&format!(
            "- {metric:?} {op:?} {threshold} (actual: {actual})\n",
            metric = f.metric,
            op = f.operator,
            threshold = f.threshold,
            actual = f.actual
        ));
    }
    s
}

fn parse_metric(s: &str) -> GateMetric {
    match s {
        "new_blocker_issues" => GateMetric::NewBlockerIssues,
        "new_high_issues" => GateMetric::NewHighIssues,
        "new_medium_issues" => GateMetric::NewMediumIssues,
        "new_warning_issues" => GateMetric::NewWarningIssues,
        "new_info_issues" => GateMetric::NewInfoIssues,
        _ => GateMetric::NewIssues,
    }
}

fn parse_operator(s: &str) -> GateOperator {
    match s {
        "gte" => GateOperator::Gte,
        "lt" => GateOperator::Lt,
        "lte" => GateOperator::Lte,
        "eq" => GateOperator::Eq,
        "ne" => GateOperator::Ne,
        _ => GateOperator::Gt,
    }
}

impl From<QualityGateConfig> for QualityGate {
    fn from(cfg: QualityGateConfig) -> Self {
        Self {
            name: cfg.name,
            conditions: cfg
                .conditions
                .into_iter()
                .map(|c| GateCondition {
                    metric: parse_metric(&c.metric),
                    operator: parse_operator(&c.op),
                    threshold: c.threshold,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(blocking: usize, warning: usize, info: usize) -> Vec<Finding> {
        let mut out = Vec::new();
        for _ in 0..blocking {
            out.push(Finding {
                detector: "test".into(),
                severity: "blocking",
                file: "x".into(),
                line: 1,
                column: 0,
                message: "b".into(),
                suggestion: None,
                evidence: None,
                codemod: None,
            });
        }
        for _ in 0..warning {
            out.push(Finding {
                detector: "test".into(),
                severity: "warning",
                file: "x".into(),
                line: 1,
                column: 0,
                message: "w".into(),
                suggestion: None,
                evidence: None,
                codemod: None,
            });
        }
        for _ in 0..info {
            out.push(Finding {
                detector: "test".into(),
                severity: "info",
                file: "x".into(),
                line: 1,
                column: 0,
                message: "i".into(),
                suggestion: None,
                evidence: None,
                codemod: None,
            });
        }
        out
    }

    #[test]
    fn default_gate_passes_clean() {
        let gate = QualityGate::default();
        let r = evaluate_gate(&gate, &sample(0, 0, 0));
        assert!(r.passed);
    }

    #[test]
    fn default_gate_fails_on_blocking() {
        let gate = QualityGate::default();
        let r = evaluate_gate(&gate, &sample(1, 0, 0));
        assert!(!r.passed);
        assert_eq!(r.failed_conditions.len(), 2); // blocker + high
    }

    #[test]
    fn default_gate_allows_few_warnings() {
        let gate = QualityGate::default();
        let r = evaluate_gate(&gate, &sample(0, 3, 0));
        assert!(r.passed);
    }

    #[test]
    fn default_gate_fails_on_too_many_warnings() {
        let gate = QualityGate::default();
        let r = evaluate_gate(&gate, &sample(0, 6, 0));
        assert!(!r.passed);
    }
}
