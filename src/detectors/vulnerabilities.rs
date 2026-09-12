use crate::detectors::Finding;
use crate::parser::ParsedFile;
use crate::registry;

const REACHABLE: &str = "reachable";

/// Detect known vulnerabilities in *imported* packages via OSV.dev.
///
/// Without a resolved package version, OSV returns *historical* vulns for the
/// package name. Those must never be `blocking` — they destroy review trust.
///
/// When reachability analysis is enabled, imports in changed code are marked
/// `reachable` (uplifted to warning for HIGH/CRITICAL advisories).
///
/// Manifest-declared (not-yet-imported) dependencies are no longer checked
/// here — `dependency_vulns` reads the manifest-pinned version and reports
/// those with accurate blocking/warning severity, so double-reporting the
/// same CVE as an unversioned `info` finding here would just be noise.
pub fn detect(parsed_files: &[ParsedFile], reachability_enabled: bool) -> Vec<Finding> {
    let mut findings = Vec::new();
    // Track which (registry, package) we've already checked to avoid duplicate API calls
    let mut checked = std::collections::HashSet::new();

    for file in parsed_files {
        let registry_name = match file.language.as_str() {
            "javascript" | "typescript" | "tsx" | "jsx" => "npm",
            "python" => "pypi",
            "rust" => "crates.io",
            _ => continue,
        };

        for import in &file.imports {
            let package = crate::detectors::extract_package_name(&import.name);
            let package = match package {
                Some(p) => p,
                None => continue,
            };

            // Skip relative imports
            if package.starts_with('.') || package.starts_with('/') {
                continue;
            }

            let key = format!("{registry_name}:{package}");
            if !checked.insert(key) {
                continue;
            }

            match registry::check_vulnerabilities(registry_name, &package) {
                Ok(vulns) => {
                    // Cap volume: at most 3 vulns per package without version pinning.
                    for vuln in vulns.iter().take(3) {
                        let severity = if reachability_enabled {
                            severity_for(vuln.severity.as_str())
                        } else {
                            "info"
                        };
                        let fixed = vuln
                            .fixed_version
                            .as_ref()
                            .map(|v| format!(" Upgrade to {v}."))
                            .unwrap_or_default();
                        findings.push(Finding {
                            detector: "vulnerabilities".to_string(),
                            severity,
                            file: file.path.clone(),
                            line: import.line,
                            column: import.column,
                            message: format!(
                                "{}: {}{} (unversioned OSV hit — confirm against lockfile)",
                                vuln.id, vuln.summary, fixed
                            ),
                            suggestion: Some(format!(
                                "Pin/check `{}` in the lockfile and upgrade if {} still applies.",
                                package, vuln.id
                            )),
                            evidence: Some(format!("{}: {}", vuln.id, vuln.summary)),
                            codemod: None,
                            confidence: reachability_enabled.then_some(5),
                            judge_rationale: None,
                            reachability: reachability_enabled.then_some(REACHABLE.to_string()),
                        });
                    }
                }
                Err(e) => {
                    findings.push(Finding {
                        detector: "vulnerabilities".to_string(),
                        severity: "info",
                        file: file.path.clone(),
                        line: import.line,
                        column: import.column,
                        message: format!(
                            "Could not check vulnerabilities for `{package}` — OSV API error: {e}"
                        ),
                        suggestion: Some(format!(
                            "Run `cargo audit` or `npm audit` manually to check `{package}` for known vulnerabilities."
                        )),
                        evidence: None,
                        codemod: None,
                        confidence: None,
                        judge_rationale: None,
                        reachability: None,
                    });
                }
            }
        }
    }

    findings
}

fn severity_for(osv_severity: &str) -> &'static str {
    match osv_severity {
        "CRITICAL" | "HIGH" => "warning",
        _ => "info",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    fn vuln(id: &str, severity: &str) -> registry::OsvVulnerability {
        registry::OsvVulnerability {
            id: id.to_string(),
            summary: "summary".to_string(),
            severity: severity.to_string(),
            fixed_version: Some("9.9.9".to_string()),
        }
    }

    #[test]
    fn imported_package_is_reachable_with_high_severity() {
        crate::registry::seed_osv_cache(
            "npm",
            "react",
            vec![vuln("GHSA-1", "HIGH"), vuln("GHSA-2", "LOW")],
        );
        let parsed = vec![parse_file("app.js", "import React from 'react';\n").unwrap()];
        let findings = detect(&parsed, true);
        let hits: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.message.contains("GHSA-"))
            .collect();
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].reachability.as_deref(), Some("reachable"));
        assert_eq!(hits[0].severity, "warning");
        assert_eq!(hits[0].confidence, Some(5));
        assert_eq!(hits[1].severity, "info");
    }

    #[test]
    fn disabled_reachability_keeps_info_and_no_reachability() {
        crate::registry::seed_osv_cache("npm", "lodash", vec![vuln("GHSA-4", "CRITICAL")]);
        let parsed = vec![parse_file("app.js", "import _ from 'lodash';\n").unwrap()];
        let findings = detect(&parsed, false);
        let hit = findings
            .iter()
            .find(|f| f.message.contains("GHSA-4"))
            .unwrap();
        assert_eq!(hit.reachability, None);
        assert_eq!(hit.severity, "info");
        assert_eq!(hit.confidence, None);
    }
}
