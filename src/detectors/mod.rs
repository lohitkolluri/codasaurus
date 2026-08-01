use crate::config::Config;
use crate::learning::store::LearningStore;
use crate::parser::ParsedFile;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

pub mod graph;
pub mod guidelines;
pub mod hallucinated_imports;
pub mod iac;
pub mod phantom_deps;
pub mod risky_patterns;
pub mod security;
pub mod slop;
pub mod stale_api;
pub mod style;
pub mod vulnerabilities;

/// Cached LearningStore — opened once and reused to avoid repeated SQLite connections.
static LEARNING_STORE: LazyLock<Mutex<Option<LearningStore>>> = LazyLock::new(|| Mutex::new(None));

/// A single finding from a detector
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// Detector name (e.g. "hallucinated-imports")
    pub detector: String,

    /// Severity: "blocking", "warning", "info"
    pub severity: &'static str,

    /// File path
    pub file: String,

    /// Line number (0 if N/A)
    pub line: usize,

    /// Column number (0 if N/A)
    pub column: usize,

    /// Human-readable message
    pub message: String,

    /// Suggested fix
    pub suggestion: Option<String>,

    /// Evidence / context snippet
    pub evidence: Option<String>,

    /// Auto-fix codemod suggestion — a code snippet to replace the issue
    #[serde(default)]
    pub codemod: Option<String>,
}

impl Finding {
    /// Stable fingerprint for deduplication and dismissal tracking.
    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.detector.as_bytes());
        hasher.update(self.file.as_bytes());
        hasher.update(self.line.to_le_bytes());
        hasher.update(self.message.as_bytes());
        hex::encode(hasher.finalize())
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Findings {
    pub findings: Vec<Finding>,
}

impl Findings {
    pub fn new() -> Self {
        Self {
            findings: Vec::new(),
        }
    }

    pub fn extend(&mut self, findings: Vec<Finding>) {
        self.findings.extend(findings);
    }

    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn has_blocking(&self) -> bool {
        self.findings.iter().any(|f| f.severity == "blocking")
    }

    pub fn count_by_severity(&self) -> HashMap<&'static str, usize> {
        let mut counts = HashMap::new();
        for f in &self.findings {
            *counts.entry(f.severity).or_insert(0) += 1;
        }
        counts
    }
}

pub fn run_all(parsed_files: &[ParsedFile], config: &Config) -> Findings {
    let mut all = Findings::new();

    if config.checks.hallucinated_imports {
        all.extend(hallucinated_imports::detect(parsed_files));
    }

    if config.checks.phantom_deps {
        all.extend(phantom_deps::detect(parsed_files));
    }

    if config.checks.secrets {
        all.extend(security::detect_secrets(parsed_files));
    }

    if config.checks.todo_leaks {
        all.extend(security::detect_todos(parsed_files));
    }

    if config.checks.over_engineering {
        all.extend(style::detect_over_engineering(parsed_files));
    }

    if config.checks.boilerplate {
        all.extend(style::detect_boilerplate(parsed_files));
    }

    if config.checks.vulnerabilities {
        all.extend(vulnerabilities::detect(parsed_files));
    }

    if config.checks.stale_api {
        all.extend(stale_api::detect(parsed_files));
    }

    if config.checks.risky_patterns {
        all.extend(risky_patterns::detect(parsed_files));
    }

    if config.checks.graph {
        all.extend(graph::detect(parsed_files));
    }

    if config.checks.iac {
        all.extend(iac::detect(parsed_files));
    }

    // Guidelines run via detect_remote on the bot path (GitHub Contents), not local git.

    // Prefer shared app DB pool (bot/serve) so dismissals stick; fall back to sidecar file.
    match LEARNING_STORE.lock() {
        Ok(mut guard) => {
            if guard.is_none() {
                *guard = if let Some(pool) = crate::bot::CONFIG_POOL.get() {
                    Some(LearningStore::from_pool(pool))
                } else {
                    LearningStore::open().ok()
                };
            }
            if let Some(ref store) = *guard {
                match store.filter_findings(&all.findings) {
                    Ok(filtered) => return Findings { findings: filtered },
                    Err(e) => {
                        eprintln!("Warning: learning store filter_findings failed: {e}; returning all unfiltered findings");
                    }
                }
            }
        }
        Err(e) => {
            eprintln!(
                "Warning: LEARNING_STORE mutex poisoned: {e}; returning all unfiltered findings"
            );
        }
    }

    all
}

/// Check if a file path matches any of the exclusion patterns.
/// Supports glob-style wildcards (`*.lock`), directory prefixes (`dist/`),
/// and direct filename/path matches.
pub fn is_excluded(path: &str, patterns: &[String]) -> bool {
    let prepared = prepare_exclude_patterns(patterns);
    is_excluded_prepared(path, &prepared)
}

/// Lowercase/trim exclusion patterns once for hot loops (many files per PR).
pub fn prepare_exclude_patterns(patterns: &[String]) -> Vec<String> {
    patterns
        .iter()
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Like [`is_excluded`] but expects patterns already lowercased via [`prepare_exclude_patterns`].
pub fn is_excluded_prepared(path: &str, patterns_lower: &[String]) -> bool {
    let path_lower = path.to_lowercase();
    patterns_lower.iter().any(|p| {
        if path_lower.ends_with(p) {
            return true;
        }
        if let Some(ext) = p.strip_prefix('*') {
            if path_lower.ends_with(ext) {
                return true;
            }
        }
        if p.ends_with('/') {
            let dir = &p[..p.len() - 1];
            if path_lower.starts_with(p) {
                return true;
            }
            if path_lower.contains(&format!("/{dir}/")) {
                return true;
            }
            if path_lower.ends_with(&format!("/{dir}")) {
                return true;
            }
        }
        false
    })
}

/// Extract the package name from an import statement. Handles @scoped/packages,
/// submodule paths, Rust :: paths, and plain package names.
pub(crate) fn extract_package_name(import: &str) -> Option<String> {
    let trimmed = import.trim();
    if trimmed.is_empty() {
        return None;
    }

    let trimmed = trimmed.strip_prefix("use ").unwrap_or(trimmed).trim();

    // Strip leading :: for Rust (e.g. "::serde::Deserialize" -> "serde::Deserialize")
    let trimmed = trimmed.strip_prefix("::").unwrap_or(trimmed);

    // Rust relative paths — these reference items within the current crate,
    // not crates.io packages, so skip them entirely.
    if trimmed.starts_with("self::")
        || trimmed.starts_with("crate::")
        || trimmed.starts_with("super::")
    {
        return None;
    }

    // @scoped/packages (npm)
    if trimmed.starts_with('@') {
        let parts: Vec<&str> = trimmed.splitn(3, '/').collect();
        if parts.len() >= 2 {
            return Some(format!("{}/{}", parts[0], parts[1]));
        }
    }

    // Rust :: paths ("std::collections::HashMap" -> "std")
    if trimmed.contains("::") {
        return trimmed.split("::").next().map(|s| s.to_string());
    }

    // npm-style submodule paths ("lodash/fp" -> "lodash")
    let parts: Vec<&str> = trimmed.splitn(2, '/').collect();
    Some(parts[0].to_string())
}
