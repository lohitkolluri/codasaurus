use crate::detectors::Finding;
use crate::parser::ParsedFile;
use crate::registry;

/// Detect known vulnerabilities in imported packages via OSV.dev
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

            let key = format!("{}:{}", registry_name, package);
            if !checked.insert(key) {
                continue;
            }

            if let Ok(vulns) = registry::check_vulnerabilities(registry_name, &package) {
                for vuln in &vulns {
                    let severity = match vuln.severity.to_lowercase().as_str() {
                        "critical" | "high" => "blocking",
                        "moderate" | "medium" => "warning",
                        _ => "info",
                    };
                    let fixed = vuln
                        .fixed_version
                        .as_ref()
                        .map(|v| format!(" Upgrade to {}.", v))
                        .unwrap_or_default();
                    findings.push(Finding {
                        detector: "vulnerabilities".to_string(),
                        severity: severity.to_string(),
                        file: file.path.clone(),
                        line: import.line,
                        column: import.column,
                        message: format!(
                            "{}: {}{}",
                            vuln.id, vuln.summary, fixed
                        ),
                        suggestion: Some(format!(
                            "Update package `{}` to the latest version to fix {}.",
                            package, vuln.id
                        )),
                        evidence: Some(format!("{}: {}", vuln.id, vuln.summary)),
                        codemod: None,
                    });
                }
            }
        }
    }

    findings
}
