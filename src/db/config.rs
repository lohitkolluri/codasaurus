use crate::db::models::*;
use crate::db::{db_execute, db_fetch_all, db_fetch_optional, DbPool};

pub async fn get_config(pool: &DbPool, key: &str) -> Result<Option<String>, sqlx::Error> {
    let result: Option<AppConfig> = db_fetch_optional!(
        pool,
        AppConfig,
        "SELECT * FROM app_config WHERE key = ?",
        key
    )?;
    Ok(result.map(|c| c.value))
}

pub async fn set_config(pool: &DbPool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    db_execute!(
        pool,
        "INSERT INTO app_config (key, value, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
        key,
        value
    )?;
    Ok(())
}

pub async fn get_all_config(pool: &DbPool) -> Result<Vec<AppConfig>, sqlx::Error> {
    db_fetch_all!(pool, AppConfig, "SELECT * FROM app_config ORDER BY key")
}

/// Dashboard keys that mirror process environment variables.
///
/// Format: `(db_key, env_key)`.
///
/// Precedence:
/// - At boot: env wins when set; otherwise DB is applied into the process env.
/// - On dashboard save: DB is updated and the process env is updated so the
///   change takes effect immediately (until the next restart, when env from
///   the host/compose again wins if set).
pub const ENV_MIRROR_KEYS: &[(&str, &str)] = &[
    ("public_url", "PUBLIC_URL"),
    ("audit_retention_days", "CODASAURUS_AUDIT_RETENTION_DAYS"),
    ("queue_workers", "CODASAURUS_QUEUE_WORKERS"),
    (
        "max_concurrent_reviews",
        "CODASAURUS_MAX_CONCURRENT_REVIEWS",
    ),
    ("hsts", "CODASAURUS_HSTS"),
    ("metrics_token", "CODASAURUS_METRICS_TOKEN"),
    ("review_timeout_secs", "CODASAURUS_REVIEW_TIMEOUT_SECS"),
    ("max_inline_comments", "CODASAURUS_MAX_INLINE_COMMENTS"),
    ("max_reviewer_files", "CODASAURUS_MAX_REVIEWER_FILES"),
    ("max_comment_bytes", "CODASAURUS_MAX_COMMENT_BYTES"),
    ("max_llm_diff_chars", "CODASAURUS_MAX_LLM_DIFF_CHARS"),
    (
        "auto_improve_max_files",
        "CODASAURUS_AUTO_IMPROVE_MAX_FILES",
    ),
    ("auto_improve_max_diff", "CODASAURUS_AUTO_IMPROVE_MAX_DIFF"),
    ("allow_local_llm", "CODASAURUS_ALLOW_LOCAL_LLM"),
    ("insecure_cookies", "CODASAURUS_INSECURE_COOKIES"),
    ("secure_cookies", "CODASAURUS_SECURE_COOKIES"),
    ("llm_daily_budget_usd", "CODASAURUS_LLM_DAILY_BUDGET_USD"),
    ("offline_mode", "CODASAURUS_OFFLINE"),
    ("jira_base_url", "JIRA_BASE_URL"),
    ("jira_email", "JIRA_EMAIL"),
    ("jira_api_token", "JIRA_API_TOKEN"),
    ("linear_api_key", "LINEAR_API_KEY"),
    ("oidc_issuer", "OIDC_ISSUER"),
    ("oidc_client_id", "OIDC_CLIENT_ID"),
    ("oidc_client_secret", "OIDC_CLIENT_SECRET"),
    ("oidc_redirect_uri", "OIDC_REDIRECT_URI"),
    ("oidc_scopes", "OIDC_SCOPES"),
    ("oidc_allow_open_join", "OIDC_ALLOW_OPEN_JOIN"),
    ("oidc_allow_unverified_email", "OIDC_ALLOW_UNVERIFIED_EMAIL"),
    ("oidc_allow_public_client", "OIDC_ALLOW_PUBLIC_CLIENT"),
];

/// Keys that require a process restart to fully apply (concurrency / worker count).
pub const RESTART_REQUIRED_KEYS: &[&str] = &["queue_workers", "max_concurrent_reviews"];

pub fn env_key_for(db_key: &str) -> Option<&'static str> {
    ENV_MIRROR_KEYS
        .iter()
        .find(|(k, _)| *k == db_key)
        .map(|(_, env)| *env)
}

/// Copy DB values into process env when the env var is unset.
pub async fn apply_db_to_env(pool: &DbPool) {
    for (db_key, env_key) in ENV_MIRROR_KEYS {
        if std::env::var(env_key).is_ok() {
            continue;
        }
        if let Ok(Some(v)) = get_config(pool, db_key).await {
            if !v.is_empty() {
                // Bridge so existing env readers pick up dashboard values.
                std::env::set_var(env_key, &v);
            }
        }
    }
}

/// Apply a dashboard setting into process env for hot reload.
pub fn apply_setting_to_env(db_key: &str, value: &str) {
    let Some(env_key) = env_key_for(db_key) else {
        return;
    };
    if value.is_empty() {
        std::env::remove_var(env_key);
    } else {
        std::env::set_var(env_key, value);
    }
}

/// Seed DB from env when the DB key is missing (ephemeral disk / compose boots).
pub async fn sync_env_mirrors_to_db(pool: &DbPool) {
    for (db_key, env_key) in ENV_MIRROR_KEYS {
        if let Ok(val) = std::env::var(env_key) {
            if val.is_empty() {
                continue;
            }
            if get_config(pool, db_key).await.ok().flatten().is_none() {
                let _ = set_config(pool, db_key, &val).await;
            }
        }
    }
}
