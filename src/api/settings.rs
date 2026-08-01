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

/// GET /api/v1/settings
async fn get_settings(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let entries = db::config::get_all_config(&state.pool).await?;

    let sensitive_keys: &[&str] = &[
        "openrouter_api_key",
        "github_private_key",
        "github_webhook_secret",
    ];

    let mut map = serde_json::Map::new();
    for entry in entries {
        let value = if sensitive_keys.contains(&entry.key.as_str()) {
            "••••••••".to_string()
        } else {
            entry.value
        };
        map.insert(entry.key, json!(value));
    }

    Ok(Json(serde_json::Value::Object(map)))
}

/// PUT /api/v1/settings/:key
async fn set_setting(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(key): Path<String>,
    Json(body): Json<SetSettingBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = super::rbac::require_owner(&state, &headers).await?;
    // Allowlist of writable config keys — prevents arbitrary config writes.
    const ALLOWED_KEYS: &[&str] = &[
        "llm_provider",
        "openrouter_api_key",
        "llm_model",
        "llm_model_cheap",
        "llm_base_url",
        "llm_daily_budget_usd",
        "public_url",
        "default_severity",
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
    ];
    if !ALLOWED_KEYS.contains(&key.as_str()) {
        return Err(ApiError::bad_request(format!("Unknown setting: {key}")));
    }
    // Don't persist the masked placeholder from GET /settings.
    if key == "openrouter_api_key"
        && (body.value.is_empty() || body.value.contains('•') || body.value.contains('*'))
    {
        return Ok(Json(
            json!({ "status": "ok", "key": key, "skipped": "unchanged" }),
        ));
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
        let allow_loopback = provider == "ollama";
        crate::ssrf::validate_llm_base_url_resolved(&body.value, allow_loopback)
            .await
            .map_err(ApiError::bad_request)?;
    }
    db::config::set_config(&state.pool, &key, &body.value).await?;
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

    Ok(Json(json!({ "status": "ok", "key": key })))
}

async fn get_github_settings(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
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
        crate::db::db_execute!(&state.pool, "DELETE FROM app_config WHERE key = ?", key)?;
    }
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
