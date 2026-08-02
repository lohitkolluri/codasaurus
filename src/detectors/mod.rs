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
pub mod license_drift;
pub mod lockfile_drift;
pub mod phantom_deps;
pub mod risky_patterns;
pub mod security;
pub mod slop;
pub mod stale_api;
pub mod style;
pub mod vulnerabilities;

/// Cached LearningStore — opened once and reused against the shared Postgres pool.
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

    /// Confidence 0-5 that the finding is a real problem (5 = certain).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,

    /// Judge rationale when an LLM judge scored this finding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge_rationale: Option<String>,
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

pub fn run_all(parsed_files: &[ParsedFile], config: &Config, repo: Option<&str>) -> Findings {
    let mut all = Findings::new();

    std::thread::scope(|s| {
        let mut handles = Vec::new();

        if config.checks.hallucinated_imports {
            handles.push(s.spawn(|| hallucinated_imports::detect(parsed_files)));
        }

        if config.checks.phantom_deps {
            handles.push(s.spawn(|| phantom_deps::detect(parsed_files)));
        }

        if config.checks.lockfile_drift {
            handles.push(s.spawn(|| lockfile_drift::detect(parsed_files)));
        }

        if config.checks.license_drift {
            handles.push(s.spawn(|| license_drift::detect(parsed_files)));
        }

        if config.checks.secrets {
            handles.push(s.spawn(|| security::detect_secrets(parsed_files)));
        }

        if config.checks.todo_leaks {
            handles.push(s.spawn(|| security::detect_todos(parsed_files)));
        }

        if config.checks.over_engineering {
            handles.push(s.spawn(|| style::detect_over_engineering(parsed_files)));
        }

        if config.checks.boilerplate {
            handles.push(s.spawn(|| style::detect_boilerplate(parsed_files)));
        }

        if config.checks.vulnerabilities {
            handles.push(s.spawn(|| vulnerabilities::detect(parsed_files)));
        }

        if config.checks.stale_api {
            handles.push(s.spawn(|| stale_api::detect(parsed_files)));
        }

        if config.checks.risky_patterns {
            handles.push(s.spawn(|| risky_patterns::detect(parsed_files)));
        }

        if config.checks.graph {
            handles.push(s.spawn(|| graph::detect(parsed_files)));
        }

        if config.checks.iac {
            handles.push(s.spawn(|| iac::detect(parsed_files)));
        }

        for handle in handles {
            match handle.join() {
                Ok(findings) => all.extend(findings),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });

    // Guidelines run via detect_remote on the bot path (GitHub Contents), not local git.

    // Prefer shared app DB pool so dismissals stick across replicas.
    match LEARNING_STORE.lock() {
        Ok(mut guard) => {
            if guard.is_none() {
                if let Some(pool) = crate::bot::CONFIG_POOL.get() {
                    *guard = Some(LearningStore::from_pool(pool));
                }
            }
            if let Some(ref store) = *guard {
                match store.filter_findings(&all.findings, repo) {
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

/// Precomputed exclude pattern (lowercased + directory path variants).
#[derive(Debug, Clone)]
pub struct PreparedExclude {
    /// Trimmed lowercase pattern as configured (e.g. `dist/`, `*.lock`).
    pub pattern: String,
    /// For directory patterns: `/{dir}/` for contains checks.
    pub dir_contains: Option<String>,
    /// For directory patterns: `/{dir}` for ends-with checks.
    pub dir_ends: Option<String>,
}

/// Check if a file path matches any of the exclusion patterns.
/// Supports glob-style wildcards (`*.lock`), directory prefixes (`dist/`),
/// and direct filename/path matches.
pub fn is_excluded(path: &str, patterns: &[String]) -> bool {
    let prepared = prepare_exclude_patterns(patterns);
    is_excluded_prepared(path, &prepared)
}

/// Lowercase/trim exclusion patterns once for hot loops (many files per PR).
/// Precomputes `/{dir}/` and `/{dir}` forms so matching does not allocate per path.
pub fn prepare_exclude_patterns(patterns: &[String]) -> Vec<PreparedExclude> {
    patterns
        .iter()
        .filter_map(|p| {
            let pattern = p.trim().to_lowercase();
            if pattern.is_empty() {
                return None;
            }
            let (dir_contains, dir_ends) = if pattern.ends_with('/') {
                let dir = &pattern[..pattern.len() - 1];
                (Some(format!("/{dir}/")), Some(format!("/{dir}")))
            } else {
                (None, None)
            };
            Some(PreparedExclude {
                pattern,
                dir_contains,
                dir_ends,
            })
        })
        .collect()
}

/// Like [`is_excluded`] but expects patterns from [`prepare_exclude_patterns`].
pub fn is_excluded_prepared(path: &str, patterns: &[PreparedExclude]) -> bool {
    let path_lower = path.to_lowercase();
    patterns.iter().any(|p| {
        if path_lower.ends_with(&p.pattern) {
            return true;
        }
        if let Some(ext) = p.pattern.strip_prefix('*') {
            if path_lower.ends_with(ext) {
                return true;
            }
        }
        if let (Some(ref contains), Some(ref ends)) = (&p.dir_contains, &p.dir_ends) {
            if path_lower.starts_with(&p.pattern) {
                return true;
            }
            if path_lower.contains(contains) {
                return true;
            }
            if path_lower.ends_with(ends) {
                return true;
            }
        }
        false
    })
}

/// Golden detector regression fixtures intentionally contain secrets/todos.
pub fn is_golden_fixture_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    lower.contains("/fixtures/golden/")
        || lower.starts_with("golden/")
        || (lower.contains("/golden/") && lower.contains("/input."))
}

/// Integration tests and golden fixtures use workspace crates and intentional samples.
pub fn is_test_or_fixture_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    is_golden_fixture_path(path)
        || lower.starts_with("tests/")
        || lower.contains("/tests/")
        || lower.contains("_test.")
        || lower.contains(".spec.")
        || lower.contains("_spec.")
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
