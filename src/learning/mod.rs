pub mod store;

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


