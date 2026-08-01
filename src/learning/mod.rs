pub mod mine;
pub mod store;

/// Action to take when a learned rule matches a finding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RuleAction {
    Ignore,
    Downgrade,
    AlwaysWarn,
}

impl RuleAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuleAction::Ignore => "ignore",
            RuleAction::Downgrade => "downgrade",
            RuleAction::AlwaysWarn => "always_warn",
        }
    }

    pub fn from_static_str(s: &str) -> Option<Self> {
        match s {
            "ignore" => Some(RuleAction::Ignore),
            "downgrade" => Some(RuleAction::Downgrade),
            "always_warn" => Some(RuleAction::AlwaysWarn),
            _ => None,
        }
    }
}

/// A rule learned from user feedback
#[derive(Debug, Clone)]
pub struct LearnedRule {
    pub id: String,
    pub detector: String,
    pub file_pattern: Option<String>,
    pub message_pattern: Option<String>,
    pub action: RuleAction,
    pub reason: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
