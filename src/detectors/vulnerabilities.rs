use crate::detectors::Finding;
use crate::parser::ParsedFile;
use crate::registry;

/// Detect known vulnerabilities in imported packages via OSV.dev.
///
/// Without a resolved package version, OSV returns *historical* vulns for the
/// package name. Those must never be `blocking` — they destroy review trust.
pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
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
                        // Unversioned query → informational only (not REQUEST_CHANGES).
                        let severity = "info";
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
                    });
                }
            }
        }
    }

    findings
}
