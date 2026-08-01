use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;

#[derive(Serialize, Deserialize, Debug, Clone, FromRow)]
pub struct Repo {
    pub id: i64,
    pub github_id: Option<i64>,
    pub full_name: String,
    pub owner: String,
    pub name: String,
    pub default_branch: Option<String>,
    pub installation_id: i64,
    pub private: bool,
    pub active: bool,
    pub config_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RepoCreate {
    pub github_id: Option<i64>,
    pub full_name: String,
    pub owner: String,
    pub name: String,
    pub default_branch: Option<String>,
    pub installation_id: i64,
    pub private: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, FromRow)]
pub struct Review {
    pub id: i64,
    pub repo_id: i64,
    pub pr_number: i64,
    pub pr_title: Option<String>,
    pub pr_author: Option<String>,
    pub pr_base_branch: Option<String>,
    pub pr_head_branch: Option<String>,
    pub pr_head_sha: Option<String>,
    pub status: String,
    pub summary_json: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReviewCreate {
    pub repo_id: i64,
    pub pr_number: i64,
    pub pr_title: Option<String>,
    pub pr_author: Option<String>,
    pub pr_base_branch: Option<String>,
    pub pr_head_branch: Option<String>,
    pub pr_head_sha: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ReviewUpdate {
    pub status: Option<String>,
    pub summary_json: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, FromRow)]
pub struct Finding {
    pub id: i64,
    pub review_id: i64,
    pub fingerprint: Option<String>,
    pub file_path: String,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
    pub column_start: Option<i64>,
    pub column_end: Option<i64>,
    pub severity: String,
    pub detector: String,
    pub rule_id: Option<String>,
    pub message: String,
    pub suggested_fix: Option<String>,
    pub code_snippet: Option<String>,
    pub context: Option<String>,
    pub category: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FindingCreate {
    pub review_id: i64,
    pub fingerprint: Option<String>,
    pub file_path: String,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
    pub column_start: Option<i64>,
    pub column_end: Option<i64>,
    pub severity: String,
    pub detector: String,
    pub rule_id: Option<String>,
    pub message: String,
    pub suggested_fix: Option<String>,
    pub code_snippet: Option<String>,
    pub context: Option<String>,
    pub category: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, FromRow)]
pub struct AuditEntry {
    pub id: i64,
    pub event_type: String,
    pub actor: Option<String>,
    pub target_type: Option<String>,
    pub target_id: Option<i64>,
    pub metadata_json: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone, FromRow)]
pub struct User {
    pub id: i64,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: String,
    #[serde(default = "default_auth_provider")]
    pub auth_provider: String,
    pub created_at: DateTime<Utc>,
}

fn default_auth_provider() -> String {
    "local".into()
}

/// Public-facing user data — never contains the password hash.
#[derive(Serialize, Debug, Clone)]
pub struct UserView {
    pub email: String,
    pub role: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, FromRow)]
pub struct AppConfig {
    pub key: String,
    pub value: String,
    pub updated_at: DateTime<Utc>,
}
