use crate::detectors::Finding;
use crate::parser::ParsedFile;
use crate::registry;
use std::collections::HashSet;
use std::sync::LazyLock;

pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut warned_registries = std::collections::HashSet::new();

    for file in parsed_files {
        if crate::detectors::is_test_or_fixture_path(&file.path) {
            continue;
        }
        let registry_name = match file.language.as_str() {
            "javascript" | "typescript" | "tsx" | "jsx" => "npm",
            "python" => "pypi",
            "rust" => "crates.io",
            "go" => "go",
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
            if registry_name == "go" && is_go_stdlib(&package) {
                continue;
            }
            match registry::check_package(registry_name, &package) {
                Ok(Some(true)) => {} // package exists
                Ok(Some(false)) => {
                    findings.push(Finding {
                        file: file.path.clone(),
                        line: import.line,
                        column: import.column,
                        severity: "blocking",
                        detector: "hallucinated-imports".to_string(),
                        message: format!(
                            "Package `{package}` not found on {registry_name}. This may be a hallucinated import."
                        ),
                        suggestion: Some(format!(
                            "Verify the correct package name at {} before installing.",
                            package_registry_url(registry_name, &package)
                        )),
                        codemod: None,
                        confidence: None,
                        judge_rationale: None,
                        evidence: None,
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
                                "Package `{package}` check skipped — registry lookup failed for {registry_name}."
                            ),
                            suggestion: Some(format!(
                                "Could not verify `{package}` on {registry_name}. Run `{}` manually to confirm.",
                                package_manual_check(registry_name, &package)
                            )),
                            evidence: None,
                            codemod: None,
                            confidence: None,
                            judge_rationale: None,
                        });
                    }
                }
            }
        }
    }

    findings
}

fn package_registry_url(registry: &str, package: &str) -> String {
    match registry {
        "npm" => format!("https://www.npmjs.com/package/{package}"),
        "pypi" => format!("https://pypi.org/project/{package}/"),
        "crates.io" => format!("https://crates.io/crates/{package}"),
        "go" => format!("https://pkg.go.dev/{package}"),
        other => format!("https://{other}/{package}"),
    }
}

fn package_manual_check(registry: &str, package: &str) -> String {
    match registry {
        "npm" => format!("npm view {package}"),
        "pypi" => format!("pip index versions {package}"),
        "crates.io" => format!("cargo search {package} --limit 1"),
        "go" => format!("go list -m {package}"),
        _ => format!("look up {package} on {registry}"),
    }
}

static NPM_BUILTINS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    // Only actual Node.js core/built-in modules. Third-party packages must be
    // verified against the registry — otherwise phantom_deps has false negatives
    // for packages like lodash, express, etc. that are NOT built-in.
    HashSet::from([
        "node:assert",
        "node:async_hooks",
        "node:buffer",
        "node:child_process",
        "node:console",
        "node:constants",
        "node:crypto",
        "node:diagnostics_channel",
        "node:dns",
        "node:events",
        "node:fs",
        "node:http",
        "node:https",
        "node:inspector",
        "node:module",
        "node:net",
        "node:os",
        "node:path",
        "node:perf_hooks",
        "node:process",
        "node:punycode",
        "node:querystring",
        "node:readline",
        "node:repl",
        "node:stream",
        "node:string_decoder",
        "node:timers",
        "node:tls",
        "node:tty",
        "node:url",
        "node:util",
        "node:v8",
        "node:vm",
        "node:wasi",
        "node:worker_threads",
        "node:zlib",
        // Without node: prefix (common in older Node.js code)
        "assert",
        "buffer",
        "child_process",
        "console",
        "constants",
        "crypto",
        "dns",
        "events",
        "fs",
        "http",
        "https",
        "module",
        "net",
        "os",
        "path",
        "punycode",
        "querystring",
        "readline",
        "repl",
        "stream",
        "string_decoder",
        "timers",
        "tls",
        "tty",
        "url",
        "util",
        "v8",
        "vm",
        "zlib",
    ])
});

static RUST_BUILTINS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| HashSet::from(["std", "core", "alloc", "proc_macro"]));

fn is_go_stdlib(path: &str) -> bool {
    let std_roots = [
        "archive",
        "bufio",
        "bytes",
        "cmp",
        "compress",
        "container",
        "context",
        "crypto",
        "database",
        "debug",
        "embed",
        "encoding",
        "errors",
        "expvar",
        "flag",
        "fmt",
        "go",
        "hash",
        "html",
        "image",
        "index",
        "io",
        "log",
        "maps",
        "math",
        "mime",
        "net",
        "os",
        "path",
        "plugin",
        "reflect",
        "regexp",
        "runtime",
        "sort",
        "strconv",
        "strings",
        "sync",
        "syscall",
        "testing",
        "text",
        "time",
        "unicode",
        "unsafe",
        "internal",
    ];
    let root = path.split('/').next().unwrap_or(path);
    std_roots.contains(&root)
}

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

#[cfg(test)]
mod url_tests {
    use super::*;

    #[test]
    fn registry_urls_are_real_hosts() {
        assert!(package_registry_url("npm", "lodash").contains("npmjs.com"));
        assert!(package_registry_url("pypi", "requests").contains("pypi.org"));
        assert!(package_registry_url("crates.io", "serde").contains("crates.io/crates/"));
        assert!(!package_registry_url("npm", "x").contains("www.npm.com"));
    }
}
