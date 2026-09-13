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
                Ok(Some(true)) => {
                    if let Some(popular) = typosquat_match(registry_name, &package) {
                        findings.push(Finding {
                            file: file.path.clone(),
                            line: import.line,
                            column: import.column,
                            severity: "warning",
                            detector: "hallucinated-imports".to_string(),
                            message: format!(
                                "Package `{package}` is one edit away from popular package `{popular}` — possible typosquat."
                            ),
                            suggestion: Some(format!(
                                "Confirm `{package}` is intentional, not a misspelling of `{popular}`."
                            )),
                            codemod: None,
                            confidence: None,
                            judge_rationale: None,
                            reachability: None,
                            evidence: None,
                        });
                    }
                }
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
            reachability: None,
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
            reachability: None,
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

/// Popular packages per registry — typosquat targets attackers actually impersonate.
static POPULAR_PACKAGES: LazyLock<std::collections::HashMap<&'static str, HashSet<&'static str>>> =
    LazyLock::new(|| {
        std::collections::HashMap::from([
            (
                "npm",
                HashSet::from([
                    "react",
                    "lodash",
                    "express",
                    "axios",
                    "chalk",
                    "commander",
                    "webpack",
                    "eslint",
                    "typescript",
                    "jest",
                    "babel",
                    "moment",
                    "request",
                    "async",
                    "underscore",
                    "debug",
                    "colors",
                    "yargs",
                    "dotenv",
                    "uuid",
                    "next",
                    "vue",
                    "prettier",
                    "mocha",
                    "socket.io",
                ]),
            ),
            (
                "pypi",
                HashSet::from([
                    "requests",
                    "numpy",
                    "pandas",
                    "flask",
                    "django",
                    "boto3",
                    "pytest",
                    "urllib3",
                    "pyyaml",
                    "setuptools",
                    "pillow",
                    "cryptography",
                    "click",
                    "jinja2",
                    "scipy",
                    "matplotlib",
                    "sqlalchemy",
                    "attrs",
                    "certifi",
                ]),
            ),
            (
                "crates.io",
                HashSet::from([
                    "serde",
                    "tokio",
                    "rand",
                    "clap",
                    "regex",
                    "reqwest",
                    "anyhow",
                    "log",
                    "thiserror",
                    "futures",
                    "chrono",
                    "bytes",
                    "hyper",
                    "syn",
                    "quote",
                    "serde_json",
                    "uuid",
                    "async-trait",
                ]),
            ),
            (
                "go",
                HashSet::from([
                    "gin-gonic/gin",
                    "gorilla/mux",
                    "sirupsen/logrus",
                    "spf13/cobra",
                    "stretchr/testify",
                    "gorm.io/gorm",
                    "google/uuid",
                    "pkg/errors",
                ]),
            ),
        ])
    });

/// Flags `package` if it's within edit distance 1-2 of a popular package it isn't.
/// Distance scales with name length so short names (e.g. "gin") don't false-positive
/// against every 4-letter package.
fn typosquat_match(registry: &str, package: &str) -> Option<&'static str> {
    let popular = POPULAR_PACKAGES.get(registry)?;
    if popular.contains(package) {
        return None;
    }
    let max_distance = if package.len() <= 4 { 1 } else { 2 };
    popular
        .iter()
        .find(|&&p| {
            let dist = levenshtein(package, p);
            dist > 0 && dist <= max_distance
        })
        .copied()
}

/// Standard Levenshtein edit distance (insert/delete/substitute), O(n*m).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
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
    fn typosquat_flags_close_misspelling() {
        assert_eq!(typosquat_match("pypi", "reqeusts"), Some("requests"));
        assert_eq!(typosquat_match("pypi", "requests"), None);
        assert_eq!(typosquat_match("npm", "lodash"), None);
    }

    #[test]
    fn registry_urls_are_real_hosts() {
        assert!(package_registry_url("npm", "lodash").contains("npmjs.com"));
        assert!(package_registry_url("pypi", "requests").contains("pypi.org"));
        assert!(package_registry_url("crates.io", "serde").contains("crates.io/crates/"));
        assert!(!package_registry_url("npm", "x").contains("www.npm.com"));
    }
}
