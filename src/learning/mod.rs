use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

mod store;
pub use store::LearningStore;

/// A rule learned from user feedback
#[derive(Debug, Clone)]
pub struct LearnedRule {
    pub id: String,
    pub detector: String,
    pub file_pattern: Option<String>,
    pub message_pattern: Option<String>,
    pub action: String, // "ignore" | "downgrade" | "always_warn"
    pub reason: String,
    pub created_at: String,
}

/// Record of a dismissed finding
#[derive(Debug, Clone)]
pub struct DismissedFinding {
    pub fingerprint: String,
    pub detector: String,
    pub file: String,
    pub line: usize,
    pub message: String,
    pub dismissed_at: String,
}
