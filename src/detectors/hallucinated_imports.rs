use crate::detectors::Finding;
use crate::parser::ParsedFile;
use crate::registry;
use std::collections::HashSet;
use std::sync::LazyLock;

pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut warned_registries = std::collections::HashSet::new();

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
                    let codemod = match registry_name {
                        "npm" => Some(format!("npm install {}", package)),
                        "pypi" => Some(format!("pip install {}", package)),
                        "crates.io" => Some(format!("cargo add {}", package)),
                        _ => None,
                    };
                    findings.push(Finding {
                        detector: "hallucinated-imports".to_string(),
                        severity: "blocking",
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
                Ok(None) | Err(_) => {
                    if warned_registries.insert(registry_name) {
                        findings.push(Finding {
                            detector: "hallucinated-imports".to_string(),
                            severity: "info",
                            file: file.path.clone(),
                            line: import.line,
                            column: import.column,
                            message: format!(
                                "Package `{}` check skipped — registry lookup failed for {}.",
                                package, registry_name
                            ),
                            suggestion: Some(format!(
                                "Could not verify `{}` on {}. Run `{} {}` manually to confirm.",
                                package,
                                registry_name,
                                match registry_name {
                                    "npm" => "npm view",
                                    "pypi" => "pip install --dry-run",
                                    _ => "check",
                                },
                                package
                            )),
                            evidence: None,
                            codemod: None,
                        });
                    }
                }
            }
        }
    }

    findings
}

static NPM_BUILTINS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    // Only actual Node.js core/built-in modules. Third-party packages must be
    // verified against the registry — otherwise phantom_deps has false negatives
    // for packages like lodash, express, etc. that are NOT built-in.
    HashSet::from([
        "node:assert", "node:async_hooks", "node:buffer", "node:child_process",
        "node:console", "node:constants", "node:crypto", "node:diagnostics_channel",
        "node:dns", "node:events", "node:fs", "node:http", "node:https",
        "node:inspector", "node:module", "node:net", "node:os", "node:path",
        "node:perf_hooks", "node:process", "node:punycode", "node:querystring",
        "node:readline", "node:repl", "node:stream", "node:string_decoder",
        "node:timers", "node:tls", "node:tty", "node:url", "node:util",
        "node:v8", "node:vm", "node:wasi", "node:worker_threads", "node:zlib",
        // Without node: prefix (common in older Node.js code)
        "assert", "buffer", "child_process", "console", "constants", "crypto",
        "dns", "events", "fs", "http", "https", "module", "net", "os", "path",
        "punycode", "querystring", "readline", "repl", "stream", "string_decoder",
        "timers", "tls", "tty", "url", "util", "v8", "vm", "zlib",
    ])
});

static RUST_BUILTINS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| HashSet::from(["std", "core", "alloc", "proc_macro"]));

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
