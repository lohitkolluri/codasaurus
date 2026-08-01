use axum::extract::{Path, State};
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::AppState;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SetSettingBody {
    pub value: String,
}

const GITHUB_KEYS: &[&str] = &[
    "github_app_id",
    "github_private_key",
    "github_webhook_secret",
    "github_app_name",
    "github_app_slug",
];

const SENSITIVE_KEYS: &[&str] = &[
    "openrouter_api_key",
    "github_private_key",
    "github_webhook_secret",
    "database_url",
    "oidc_client_secret",
    "metrics_token",
    "jira_api_token",
    "linear_api_key",
];

/// Writable config keys — prevents arbitrary config writes.
const ALLOWED_KEYS: &[&str] = &[
    "llm_provider",
    "openrouter_api_key",
    "llm_model",
    "llm_model_cheap",
    "llm_base_url",
    "llm_daily_budget_usd",
    "public_url",
    "default_severity",
    "review_strictness",
    "max_warnings",
    "max_blocking",
    "forbidden_paths",
    "request_reviewers",
    "create_check_run",
    "hallucinated_imports_enabled",
    "phantom_deps_enabled",
    "vulnerabilities_enabled",
    "secrets_enabled",
    "over_engineering_enabled",
    "boilerplate_enabled",
    "todo_leaks_enabled",
    "stale_api_enabled",
    "risky_patterns_enabled",
    "graph_enabled",
    "guidelines_enabled",
    "iac_enabled",
    "auto_labels_enabled",
    "exclude_patterns",
    "update_pr_description",
    "custom_instructions",
    "allow_auto_fix",
    "offline_mode",
    // Runtime / ops (env mirrors)
    "audit_retention_days",
    "queue_workers",
    "max_concurrent_reviews",
    "hsts",
    "metrics_token",
    "review_timeout_secs",
    "max_inline_comments",
    "max_reviewer_files",
    "max_comment_bytes",
    "max_llm_diff_chars",
    "auto_improve_max_files",
    "auto_improve_max_diff",
    "allow_local_llm",
    "insecure_cookies",
    "secure_cookies",
    // Ticket integrations
    "jira_base_url",
    "jira_email",
    "jira_api_token",
    "linear_api_key",
    // OIDC / SSO
    "oidc_issuer",
    "oidc_client_id",
    "oidc_client_secret",
    "oidc_redirect_uri",
    "oidc_scopes",
    "oidc_allow_open_join",
    "oidc_allow_unverified_email",
    "oidc_allow_public_client",
];

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_settings))
        .route("/{key}", put(set_setting))
        .route("/github", get(get_github_settings))
        .route("/github", delete(delete_github_settings))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn mask_if_sensitive(key: &str, value: String) -> String {
    if SENSITIVE_KEYS.contains(&key) && !value.is_empty() {
        "••••••••".to_string()
    } else {
        value
    }
}

/// GET /api/v1/settings
async fn get_settings(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;
    let entries = db::config::get_all_config(&state.pool).await?;

    // Never expose connection strings to the dashboard.
    const HIDDEN_KEYS: &[&str] = &["database_url"];

    let mut map = serde_json::Map::new();
    for entry in entries {
        if HIDDEN_KEYS.contains(&entry.key.as_str()) {
            continue;
        }
        let value = mask_if_sensitive(&entry.key, entry.value);
        map.insert(entry.key, json!(value));
    }

    // Fill missing mirrors from process env so the form shows effective values.
    for (db_key, env_key) in db::config::ENV_MIRROR_KEYS {
        if map.contains_key(*db_key) {
            continue;
        }
        if let Ok(val) = std::env::var(env_key) {
            if !val.is_empty() {
                map.insert((*db_key).to_string(), json!(mask_if_sensitive(db_key, val)));
            }
        }
    }

    Ok(Json(serde_json::Value::Object(map)))
}

fn parse_positive_int(value: &str, key: &str, min: u64, max: u64) -> Result<(), ApiError> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let n: u64 = value
        .trim()
        .parse()
        .map_err(|_| ApiError::bad_request(format!("{key} must be an integer")))?;
    if n < min || n > max {
        return Err(ApiError::bad_request(format!(
            "{key} must be between {min} and {max}"
        )));
    }
    Ok(())
}

fn parse_boolish(value: &str, key: &str) -> Result<(), ApiError> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let v = value.trim().to_ascii_lowercase();
    if matches!(
        v.as_str(),
        "1" | "0" | "true" | "false" | "yes" | "no" | "on" | "off"
    ) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "{key} must be a boolean (true/false or 1/0)"
        )))
    }
}

/// PUT /api/v1/settings/:key
async fn set_setting(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(key): Path<String>,
    Json(body): Json<SetSettingBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = super::rbac::require_owner(&state, &headers).await?;
    if !ALLOWED_KEYS.contains(&key.as_str()) {
        return Err(ApiError::bad_request(format!("Unknown setting: {key}")));
    }
    // Don't persist the masked placeholder from GET /settings.
    if SENSITIVE_KEYS.contains(&key.as_str())
        && (body.value.is_empty() || body.value.contains('•') || body.value.contains('*'))
    {
        return Ok(Json(
            json!({ "status": "ok", "key": key, "skipped": "unchanged" }),
        ));
    }
    if key == "review_strictness" {
        let v = body.value.trim().to_ascii_lowercase();
        if !matches!(
            v.as_str(),
            "lenient" | "balanced" | "strict" | "nitpick" | ""
        ) {
            return Err(ApiError::bad_request(
                "review_strictness must be lenient|balanced|strict|nitpick",
            ));
        }
    }
    if key == "llm_daily_budget_usd" && !body.value.trim().is_empty() {
        body.value
            .trim()
            .parse::<f64>()
            .map_err(|_| ApiError::bad_request("llm_daily_budget_usd must be a number"))?;
    }
    if key == "llm_base_url" && !body.value.is_empty() {
        let provider = db::config::get_config(&state.pool, "llm_provider")
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        let from_db = db::config::get_config(&state.pool, "allow_local_llm")
            .await
            .ok()
            .flatten()
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        let from_env = std::env::var("CODASAURUS_ALLOW_LOCAL_LLM")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let allow_loopback = provider == "ollama" || from_db || from_env;
        crate::ssrf::validate_llm_base_url_resolved(&body.value, allow_loopback)
            .await
            .map_err(ApiError::bad_request)?;
    }

    match key.as_str() {
        "audit_retention_days" => parse_positive_int(&body.value, &key, 7, 730)?,
        "queue_workers" => parse_positive_int(&body.value, &key, 1, 8)?,
        "max_concurrent_reviews" => parse_positive_int(&body.value, &key, 1, 64)?,
        "review_timeout_secs" => parse_positive_int(&body.value, &key, 30, 3600)?,
        "max_inline_comments" => parse_positive_int(&body.value, &key, 1, 100)?,
        "max_reviewer_files" => parse_positive_int(&body.value, &key, 1, 200)?,
        "max_comment_bytes" => parse_positive_int(&body.value, &key, 1000, 500_000)?,
        "max_llm_diff_chars" => parse_positive_int(&body.value, &key, 500, 200_000)?,
        "auto_improve_max_files" => parse_positive_int(&body.value, &key, 1, 500)?,
        "auto_improve_max_diff" => parse_positive_int(&body.value, &key, 1000, 500_000)?,
        "hsts"
        | "allow_local_llm"
        | "insecure_cookies"
        | "secure_cookies"
        | "oidc_allow_open_join"
        | "oidc_allow_unverified_email"
        | "oidc_allow_public_client"
        | "offline_mode" => parse_boolish(&body.value, &key)?,
        "public_url" | "oidc_issuer" | "oidc_redirect_uri" | "jira_base_url"
            if !body.value.trim().is_empty() =>
        {
            let v = body.value.trim();
            if !(v.starts_with("http://") || v.starts_with("https://")) {
                return Err(ApiError::bad_request(format!(
                    "{key} must be an http(s) URL"
                )));
            }
        }
        _ => {}
    }

    db::config::set_config(&state.pool, &key, &body.value).await?;
    db::config::apply_setting_to_env(&key, &body.value);
    db::audit::log_event(
        &state.pool,
        "settings.updated",
        Some(&actor.email),
        Some(&key),
        None,
    )
    .await;
    if key == "offline_mode" {
        let on = matches!(
            body.value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        );
        crate::registry::set_offline_mode(on);
    }

    let restart_required = db::config::RESTART_REQUIRED_KEYS.contains(&key.as_str());
    Ok(Json(json!({
        "status": "ok",
        "key": key,
        "restart_required": restart_required,
    })))
}

async fn get_github_settings(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;
    let app_id = db::config::get_config(&state.pool, "github_app_id")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("GITHUB_APP_ID").ok());

    let app_name = db::config::get_config(&state.pool, "github_app_name")
        .await
        .ok()
        .flatten();

    let slug = db::config::get_config(&state.pool, "github_app_slug")
        .await
        .ok()
        .flatten();

    Ok(Json(json!({
        "app_id": app_id,
        "app_name": app_name,
        "slug": slug,
        "configured": app_id.is_some(),
    })))
}

async fn delete_github_settings(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = super::rbac::require_owner(&state, &headers).await?;
    for key in GITHUB_KEYS {
        db::config::delete_config(&state.pool, key).await?;
    }
    // Drop live credentials so webhooks fail closed and workers stop minting tokens.
    crate::bot::clear_bot_config();
    // Without App credentials we cannot talk to GitHub; stop reviewing locally.
    let _ = crate::db::db_execute!(&state.pool, "UPDATE repos SET active = false");
    db::audit::log_event(
        &state.pool,
        "github.config_cleared",
        Some(&actor.email),
        Some("github_app"),
        None,
    )
    .await;
    Ok(Json(json!({
        "status": "ok",
        "message": "Local GitHub App config cleared and repos marked inactive. This does not uninstall the App on GitHub. Remove it under GitHub Settings → Applications if needed."
    })))
}
