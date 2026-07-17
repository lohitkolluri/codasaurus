use crate::config::Config;
use crate::learning::store::LearningStore;
use crate::parser::ParsedFile;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod graph;
pub mod guidelines;
pub mod hallucinated_imports;
pub mod phantom_deps;
pub mod security;
pub mod stale_api;
pub mod style;
pub mod vulnerabilities;

/// A single finding from a detector
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// Detector name (e.g. "hallucinated-imports")
    pub detector: String,

    /// Severity: "blocking", "warning", "info"
    pub severity: String,

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
    /// Stable fingerprint for deduplication and dismissal tracking
    #[allow(dead_code)]
    pub fn fingerprint(&self) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};
        self.detector.hash(&mut hasher);
        self.file.hash(&mut hasher);
        self.line.hash(&mut hasher);
        self.message.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

/// Collection of findings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Findings {
    pub findings: Vec<Finding>,
}

impl Findings {
    pub fn new() -> Self {
        Self {
            findings: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn add(&mut self, finding: Finding) {
        self.findings.push(finding);
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

    #[allow(dead_code)]
    pub fn has_warnings(&self) -> bool {
        self.findings.iter().any(|f| f.severity == "warning")
    }

    pub fn count_by_severity(&self) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        for f in &self.findings {
            *counts.entry(f.severity.clone()).or_insert(0) += 1;
        }
        counts
    }
}

/// Run all enabled detectors on the parsed files
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

    // Graph/blast radius detector (always on when graph module is enabled)
    all.extend(graph::detect(parsed_files, config));

    all.extend(guidelines::detect(config));

    // Filter findings through the learning store (user dismissals + learned rules)
    if let Ok(store) = LearningStore::open() {
        if let Ok(filtered) = store.filter_findings(&all.findings) {
            return Findings { findings: filtered };
        }
    }

    all
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
    if trimmed.starts_with("self::") || trimmed.starts_with("crate::") || trimmed.starts_with("super::") {
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

/// Run LLM-based review on the parsed files
#[allow(dead_code)]
pub async fn run_llm(parsed_files: &[ParsedFile], _config: &Config) -> Findings {
    let diff: String = parsed_files
        .iter()
        .map(|f| format!("--- {}\n{}\n", f.path, f.raw_content))
        .collect::<Vec<_>>()
        .join("\n");

    let Some(llm_cfg) = crate::llm::LlmConfig::from_env() else {
        return Findings::new();
    };

    let Ok(output) = crate::llm::review_diff(&diff, &llm_cfg, None).await else {
        return Findings::new();
    };

    let findings: Vec<Finding> = output
        .issues
        .into_iter()
        .map(|i| Finding {
            detector: "llm".into(),
            severity: i.severity,
            file: i.file,
            line: i.line,
            column: 0,
            message: i.description,
            suggestion: i.suggestion,
            evidence: None,
            codemod: None,
        })
        .collect();

    Findings { findings }
}
