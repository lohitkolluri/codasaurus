use crate::detectors::{Finding, Findings};
use crate::parser::ParsedFile;
use crate::registry;

/// Detect imports that reference packages not found in package registries
pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for file in parsed_files {
        let registry_name = match file.language.as_str() {
            "javascript" | "typescript" | "tsx" | "jsx" => "npm",
            "python" => "pypi",
            "rust" => "crates.io",
            _ => continue, // unsupported language for now
        };

        for import in &file.imports {
            let package = extract_package_name(&import.name, registry_name);
            let package = match package {
                Some(p) => p,
                None => continue,
            };

            // Skip relative imports and built-ins
            if package.starts_with('.') || package.starts_with('/') {
                continue;
            }

            // Check if this is a known built-in
            if is_builtin(&package, registry_name) {
                continue;
            }

            match registry::check_package(registry_name, &package) {
                Ok(Some(true)) => {} // package exists
                Ok(Some(false)) => {
                    findings.push(Finding {
                        detector: "hallucinated-imports".to_string(),
                        severity: "blocking".to_string(),
                        file: file.path.clone(),
                        line: import.line,
                        column: import.column,
                        message: format!(
                            "Package `{}` not found on {}. This may be a hallucinated import.",
                            package, registry_name
                        ),
                        suggestion: Some(format!(
                            "Check the correct package name on {} and install it with the appropriate package manager.",
                            registry_name
                        )),
                        evidence: Some(import.name.clone()),
                    });
                }
                Ok(None) => {} // couldn't check (network error, etc.)
                Err(_) => {}   // error during check
            }
        }
    }

    findings
}

/// Extract the package name from an import statement
fn extract_package_name(import: &str, registry: &str) -> Option<String> {
    let trimmed = import.trim();

    match registry {
        "npm" => {
            // Handle: 'lodash', '@scope/pkg', 'lodash/map'
            if trimmed.starts_with('@') {
                // Scoped package: @scope/pkg or @scope/pkg/sub
                let parts: Vec<&str> = trimmed.splitn(3, '/').collect();
                if parts.len() >= 2 {
                    Some(format!("{}/{}", parts[0], parts[1]))
                } else {
                    None
                }
            } else {
                // Unscoped: pkg or pkg/subpath
                let parts: Vec<&str> = trimmed.splitn(2, '/').collect();
                Some(parts[0].to_string())
            }
        }
        "pypi" => {
            // Python: package_name or package_name.submodule
            let parts: Vec<&str> = trimmed.splitn(2, '.').collect();
            Some(parts[0].to_string())
        }
        "crates.io" => {
            // Rust: crate::module or ::crate
            let clean = trimmed.trim_start_matches("::");
            let parts: Vec<&str> = clean.splitn(2, "::").collect();
            Some(parts[0].to_string())
        }
        _ => Some(trimmed.to_string()),
    }
}

/// Known built-in packages for common languages
fn is_builtin(package: &str, registry: &str) -> bool {
    match registry {
        "npm" => matches!(
            package,
            "react"
                | "vue"
                | "express"
                | "lodash"
                | "axios"
                | "typescript"
                | "next"
                | "webpack"
                | "vite"
                | "jest"
                | "mocha"
                | "chai"
                | "moment"
                | "date-fns"
                | "uuid"
                | "node:fs"
                | "node:path"
                | "node:http"
                | "node:crypto"
                | "node:os"
                | "node:stream"
                | "node:buffer"
                | "node:child_process"
                | "node:util"
                | "node:events"
                | "node:url"
                | "node:querystring"
                | "node:assert"
        ),
        _ => false,
    }
}
