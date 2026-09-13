//! SARIF 2.1.0 export of Tier-1 (deterministic) findings, for interop with
//! GitHub code scanning and other SARIF consumers. This is a format
//! conversion only — it does not add detection depth beyond what `Findings`
//! already reports.

use super::Findings;
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// The subset of finding fields needed to render one SARIF result.
/// Implemented for both the in-process `Finding` and the persisted DB row
/// so the same renderer serves live detector output and stored review findings.
pub struct SarifFinding<'a> {
    pub detector: &'a str,
    pub severity: &'a str,
    pub file: &'a str,
    pub line: i64,
    pub column: i64,
    pub message: &'a str,
    pub confidence: Option<i64>,
    pub reachability: Option<&'a str>,
}

impl<'a> From<&'a super::Finding> for SarifFinding<'a> {
    fn from(f: &'a super::Finding) -> Self {
        Self {
            detector: &f.detector,
            severity: f.severity,
            file: &f.file,
            line: f.line as i64,
            column: f.column as i64,
            message: &f.message,
            confidence: f.confidence.map(i64::from),
            reachability: f.reachability.as_deref(),
        }
    }
}

impl<'a> From<&'a crate::db::models::Finding> for SarifFinding<'a> {
    fn from(f: &'a crate::db::models::Finding) -> Self {
        Self {
            detector: &f.detector,
            severity: &f.severity,
            file: &f.file_path,
            line: f.line_start.unwrap_or(0) as i64,
            column: f.column_start.unwrap_or(0) as i64,
            message: &f.message,
            confidence: f.confidence.map(i64::from),
            reachability: None,
        }
    }
}

fn sarif_level(severity: &str) -> &'static str {
    match severity {
        "blocking" => "error",
        "warning" => "warning",
        _ => "note",
    }
}

/// Convert findings into a `sarifLog` document for one run.
pub fn findings_to_sarif(findings: &Findings, repo_full_name: &str, commit_sha: &str) -> Value {
    let converted: Vec<SarifFinding> = findings.findings.iter().map(SarifFinding::from).collect();
    sarif_from(&converted, repo_full_name, commit_sha)
}

/// Convert persisted DB findings (as returned by `db::reviews::get_findings_for_review`)
/// into a `sarifLog` document.
pub fn db_findings_to_sarif(
    findings: &[crate::db::models::Finding],
    repo_full_name: &str,
    commit_sha: &str,
) -> Value {
    let converted: Vec<SarifFinding> = findings.iter().map(SarifFinding::from).collect();
    sarif_from(&converted, repo_full_name, commit_sha)
}

fn sarif_from(findings: &[SarifFinding], repo_full_name: &str, commit_sha: &str) -> Value {
    let rule_ids: BTreeSet<&str> = findings.iter().map(|f| f.detector).collect();

    let rules: Vec<Value> = rule_ids
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "shortDescription": { "text": format!("codasaurus: {id}") },
            })
        })
        .collect();

    let results: Vec<Value> = findings
        .iter()
        .map(|f| {
            let mut properties = serde_json::Map::new();
            if let Some(c) = f.confidence {
                properties.insert("confidence".into(), json!(c));
            }
            if let Some(r) = f.reachability {
                properties.insert("reachability".into(), json!(r));
            }

            let mut result = json!({
                "ruleId": f.detector,
                "level": sarif_level(f.severity),
                "message": { "text": f.message.chars().take(2000).collect::<String>() },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": f.file },
                        "region": {
                            "startLine": f.line.max(1),
                            "startColumn": f.column.max(1),
                        }
                    }
                }],
            });
            if !properties.is_empty() {
                result["properties"] = Value::Object(properties);
            }
            result
        })
        .collect();

    json!({
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/Schemata/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "codasaurus",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/accuknox/codasaurus",
                    "rules": rules,
                }
            },
            "originalUriBaseIds": {
                "REPO_ROOT": { "uri": format!("https://github.com/{repo_full_name}/blob/{commit_sha}/") }
            },
            "results": results,
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::Finding;

    #[test]
    fn maps_severity_and_location() {
        let findings = Findings {
            findings: vec![Finding {
                detector: "hallucinated-imports".into(),
                severity: "blocking",
                file: "src/main.rs".into(),
                line: 10,
                column: 3,
                message: "imports a package that doesn't exist".into(),
                suggestion: None,
                evidence: None,
                codemod: None,
                confidence: Some(5),
                judge_rationale: None,
                reachability: Some("reachable".into()),
            }],
        };

        let sarif = findings_to_sarif(&findings, "acme/widgets", "deadbeef");
        assert_eq!(sarif["version"], "2.1.0");
        let run = &sarif["runs"][0];
        assert_eq!(
            run["tool"]["driver"]["rules"][0]["id"],
            "hallucinated-imports"
        );
        let result = &run["results"][0];
        assert_eq!(result["level"], "error");
        assert_eq!(
            result["locations"][0]["physicalLocation"]["region"]["startLine"],
            10
        );
        assert_eq!(result["properties"]["confidence"], 5);
    }

    #[test]
    fn zero_line_clamps_to_one() {
        let findings = Findings {
            findings: vec![Finding {
                detector: "phantom-deps".into(),
                severity: "info",
                file: "Cargo.toml".into(),
                line: 0,
                column: 0,
                message: "unused dependency".into(),
                suggestion: None,
                evidence: None,
                codemod: None,
                confidence: None,
                judge_rationale: None,
                reachability: None,
            }],
        };
        let sarif = findings_to_sarif(&findings, "acme/widgets", "deadbeef");
        let region = &sarif["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 1);
        assert_eq!(region["startColumn"], 1);
    }
}
