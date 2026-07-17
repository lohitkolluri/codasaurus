use crate::detectors::Finding;
use crate::parser::ParsedFile;
use crate::registry;
use once_cell::sync::Lazy;
use std::collections::HashSet;

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
            let package = crate::detectors::extract_package_name(&import.name);
            let package = match package {
                Some(p) => p,
                None => continue,
            };

            // Skip relative imports and built-ins
            if package.starts_with('.') || package.starts_with('/') {
                continue;
            }

            if is_builtin(&package, registry_name) {
                continue;
            }

            match registry::check_package(registry_name, &package) {
                Ok(Some(true)) => {} // package exists
                Ok(Some(false)) => {
                    let correct_name = if import.name.contains('/') {
                        import
                            .name
                            .split('/')
                            .next_back()
                            .unwrap_or(&package)
                            .to_string()
                    } else {
                        package.clone()
                    };
                    let codemod = match registry_name {
                        "npm" => Some(format!("npm install {}", correct_name)),
                        "pypi" => Some(format!("pip install {}", correct_name)),
                        "crates.io" => Some(format!("cargo add {}", correct_name)),
                        _ => None,
                    };
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
                        codemod,
                    });
                }
                Ok(None) => {} // couldn't check (network error, etc.)
                Err(_) => {}   // error during check
            }
        }
    }

    findings
}

static NPM_BUILTINS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    HashSet::from([
        "react",
        "vue",
        "express",
        "lodash",
        "axios",
        "typescript",
        "next",
        "webpack",
        "vite",
        "jest",
        "mocha",
        "chai",
        "moment",
        "date-fns",
        "uuid",
        "node:fs",
        "node:path",
        "node:http",
        "node:crypto",
        "node:os",
        "node:stream",
        "node:buffer",
        "node:child_process",
        "node:util",
        "node:events",
        "node:url",
        "node:querystring",
        "node:assert",
    ])
});

static RUST_BUILTINS: Lazy<HashSet<&'static str>> =
    Lazy::new(|| HashSet::from(["std", "core", "alloc", "proc_macro"]));

/// Known built-in packages for common languages
pub(crate) fn is_builtin(package: &str, registry: &str) -> bool {
    match registry {
        "npm" => NPM_BUILTINS.contains(package),
        "crates.io" => {
            if RUST_BUILTINS.contains(package) {
                return true;
            }
            // Defense in depth: handle raw module paths when extract_package_name
            // hasn't split on :: yet (e.g. "std::collections::HashMap")
            package.starts_with("std::")
                || package.starts_with("core::")
                || package.starts_with("alloc::")
                || package.starts_with("proc_macro::")
        }
        _ => false,
    }
}
