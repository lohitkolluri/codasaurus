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
    /// Optional repo override for review personality.
    pub review_strictness: Option<String>,
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
            review_strictness: None,
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
    #[serde(default)]
    review_strictness: Option<String>,
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
            // One snapshot (cache-warmed) instead of six keyed round-trips.
            if let Ok(entries) = crate::db::config::get_all_config(pool).await {
                let map: std::collections::HashMap<&str, &str> = entries
                    .iter()
                    .map(|e| (e.key.as_str(), e.value.as_str()))
                    .collect();
                if let Some(v) = map.get("default_severity") {
                    if matches!(*v, "blocking" | "warning" | "info") {
                        pack.min_severity = (*v).to_string();
                    }
                }
                if let Some(v) = map.get("max_warnings") {
                    if let Ok(n) = v.parse() {
                        pack.max_warnings = n;
                    }
                }
                if let Some(v) = map.get("max_blocking") {
                    if let Ok(n) = v.parse() {
                        pack.max_blocking = n;
                    }
                }
                if let Some(v) = map.get("forbidden_paths") {
                    pack.forbidden_paths = parse_path_list(v);
                }
                if let Some(v) = map.get("request_reviewers") {
                    pack.request_reviewers = parse_bool(v, true);
                }
                if let Some(v) = map.get("create_check_run") {
                    pack.create_check_run = parse_bool(v, true);
                }
            }
            // review_strictness is applied by the caller via strictness::apply_to_pack
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
                        if let Some(s) = pj.review_strictness {
                            if !s.trim().is_empty() {
                                pack.review_strictness = Some(s);
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
            // Path-boundary match only — substring would false-positive (`src` ⊂ `source`).
            let hit = if rule.ends_with('/') {
                path_norm.starts_with(rule) || path_norm == rule.trim_end_matches('/')
            } else {
                path_norm == rule
                    || path_norm.starts_with(&format!("{rule}/"))
                    || path_norm.rsplit('/').next() == Some(rule)
            };
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
                    confidence: None,
                    judge_rationale: None,
                    reachability: None,
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
            confidence: None,
            judge_rationale: None,
            reachability: None,
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
            confidence: None,
            judge_rationale: None,
            reachability: None,
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
    fn forbidden_does_not_substring_match() {
        let f = forbidden_path_findings(
            &["tests/source.rs".into(), "src/main.rs".into()],
            &["src".into()],
        );
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].file, "src/main.rs");
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
                confidence: None,
                judge_rationale: None,
                reachability: None,
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
