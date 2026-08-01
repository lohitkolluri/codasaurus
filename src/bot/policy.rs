//! Policy packs: severity floors, budgets, forbidden paths.

use crate::detectors::Finding;
use serde::Deserialize;

/// Org/repo policy applied during PR review.
#[derive(Debug, Clone)]
pub struct PolicyPack {
    pub min_severity: String,
    pub max_blocking: usize,
    pub max_warnings: usize,
    pub forbidden_paths: Vec<String>,
    pub request_reviewers: bool,
    pub create_check_run: bool,
}

impl Default for PolicyPack {
    fn default() -> Self {
        Self {
            min_severity: "info".into(),
            max_blocking: 0,
            max_warnings: 50,
            forbidden_paths: Vec::new(),
            request_reviewers: true,
            create_check_run: true,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct PolicyJson {
    #[serde(default)]
    max_blocking: Option<usize>,
    #[serde(default)]
    max_warnings: Option<usize>,
    #[serde(default)]
    forbidden_paths: Option<Vec<String>>,
    #[serde(default)]
    request_reviewers: Option<bool>,
    #[serde(default)]
    create_check_run: Option<bool>,
    #[serde(default)]
    min_severity: Option<String>,
}

impl PolicyPack {
    /// Load from dashboard DB keys + optional repo `config_json.policy`.
    pub async fn load(
        pool: Option<&crate::db::DbPool>,
        repo_config_json: Option<&str>,
        pre_merge_max_blocking: usize,
        pre_merge_max_warnings: usize,
    ) -> Self {
        let mut pack = Self {
            max_blocking: pre_merge_max_blocking,
            max_warnings: pre_merge_max_warnings,
            ..Self::default()
        };

        if let Some(pool) = pool {
            if let Ok(Some(v)) = crate::db::config::get_config(pool, "default_severity").await {
                if matches!(v.as_str(), "blocking" | "warning" | "info") {
                    pack.min_severity = v;
                }
            }
            if let Ok(Some(v)) = crate::db::config::get_config(pool, "max_warnings").await {
                if let Ok(n) = v.parse() {
                    pack.max_warnings = n;
                }
            }
            if let Ok(Some(v)) = crate::db::config::get_config(pool, "max_blocking").await {
                if let Ok(n) = v.parse() {
                    pack.max_blocking = n;
                }
            }
            if let Ok(Some(v)) = crate::db::config::get_config(pool, "forbidden_paths").await {
                pack.forbidden_paths = parse_path_list(&v);
            }
            if let Ok(Some(v)) = crate::db::config::get_config(pool, "request_reviewers").await {
                pack.request_reviewers = parse_bool(&v, true);
            }
            if let Ok(Some(v)) = crate::db::config::get_config(pool, "create_check_run").await {
                pack.create_check_run = parse_bool(&v, true);
            }
        }

        if let Some(raw) = repo_config_json {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
                if let Some(p) = value.get("policy") {
                    if let Ok(pj) = serde_json::from_value::<PolicyJson>(p.clone()) {
                        if let Some(n) = pj.max_blocking {
                            pack.max_blocking = n;
                        }
                        if let Some(n) = pj.max_warnings {
                            pack.max_warnings = n;
                        }
                        if let Some(paths) = pj.forbidden_paths {
                            pack.forbidden_paths = paths;
                        }
                        if let Some(b) = pj.request_reviewers {
                            pack.request_reviewers = b;
                        }
                        if let Some(b) = pj.create_check_run {
                            pack.create_check_run = b;
                        }
                        if let Some(s) = pj.min_severity {
                            if matches!(s.as_str(), "blocking" | "warning" | "info") {
                                pack.min_severity = s;
                            }
                        }
                    }
                }
                // Flat keys also accepted on config_json for convenience.
                if let Some(paths) = value.get("forbidden_paths").and_then(|v| v.as_array()) {
                    pack.forbidden_paths = paths
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect();
                }
            }
        }

        pack
    }
}

fn parse_bool(v: &str, default: bool) -> bool {
    match v.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => true,
        "false" | "0" | "no" | "off" => false,
        _ => default,
    }
}

fn parse_path_list(v: &str) -> Vec<String> {
    let trimmed = v.trim();
    if trimmed.starts_with('[') {
        if let Ok(arr) = serde_json::from_str::<Vec<String>>(trimmed) {
            return arr;
        }
    }
    trimmed
        .split(&[',', '\n'][..])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Emit blocking findings for forbidden path touches.
pub fn forbidden_path_findings(changed_paths: &[String], forbidden: &[String]) -> Vec<Finding> {
    if forbidden.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for path in changed_paths {
        for rule in forbidden {
            let rule = rule.trim().trim_start_matches('/');
            if rule.is_empty() {
                continue;
            }
            let path_norm = path.trim_start_matches('/');
            let hit = path_norm == rule
                || path_norm.starts_with(&format!("{rule}/"))
                || path_norm.contains(rule);
            if hit {
                out.push(Finding {
                    detector: "policy".to_string(),
                    severity: "blocking",
                    file: path.clone(),
                    line: 1,
                    column: 0,
                    message: format!("Path `{path}` matches forbidden policy pattern `{rule}`"),
                    suggestion: Some(
                        "Remove this file from the PR or update the org policy pack.".into(),
                    ),
                    evidence: None,
                    codemod: None,
                });
                break;
            }
        }
    }
    out
}

/// If warning/blocking counts exceed policy caps, add a synthetic blocking finding.
pub fn enforce_count_caps(findings: &mut Vec<Finding>, pack: &PolicyPack) {
    let blocking = findings.iter().filter(|f| f.severity == "blocking").count();
    let warning = findings.iter().filter(|f| f.severity == "warning").count();

    if blocking > pack.max_blocking {
        findings.push(Finding {
            detector: "policy".to_string(),
            severity: "blocking",
            file: "POLICY".into(),
            line: 0,
            column: 0,
            message: format!(
                "Policy: {blocking} blocking findings exceed max_blocking={}",
                pack.max_blocking
            ),
            suggestion: Some("Fix blocking issues or raise max_blocking in policy.".into()),
            evidence: None,
            codemod: None,
        });
    }
    if warning > pack.max_warnings {
        findings.push(Finding {
            detector: "policy".to_string(),
            severity: "blocking",
            file: "POLICY".into(),
            line: 0,
            column: 0,
            message: format!(
                "Policy: {warning} warnings exceed max_warnings={}",
                pack.max_warnings
            ),
            suggestion: Some("Address warnings or raise max_warnings in policy.".into()),
            evidence: None,
            codemod: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_matches_prefix() {
        let f = forbidden_path_findings(
            &["secrets/prod.key".into(), "src/main.rs".into()],
            &["secrets/".into()],
        );
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].file, "secrets/prod.key");
    }

    #[test]
    fn caps_add_policy_finding() {
        let mut findings = vec![
            Finding {
                detector: "todo-leaks".into(),
                severity: "warning",
                file: "a.rs".into(),
                line: 1,
                column: 0,
                message: "t".into(),
                suggestion: None,
                evidence: None,
                codemod: None,
            };
            3
        ];
        let pack = PolicyPack {
            max_warnings: 1,
            ..Default::default()
        };
        enforce_count_caps(&mut findings, &pack);
        assert!(findings.iter().any(|f| f.detector == "policy"));
    }
}
