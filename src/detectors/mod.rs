use crate::config::Config;
use crate::parser::ParsedFile;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod hallucinated_imports;
pub mod phantom_deps;
pub mod security;
pub mod style;

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

    all
}
