use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
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
        .route("/github/rotate", post(rotate_github_credentials))
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
    Path(key): Path<String>,
    Json(body): Json<SetSettingBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Allowlist of writable config keys — prevents arbitrary config writes.
    const ALLOWED_KEYS: &[&str] = &[
        "llm_provider",
        "openrouter_api_key",
        "llm_model",
        "llm_base_url",
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
        "graph_enabled",
        "guidelines_enabled",
        "iac_enabled",
        "auto_labels_enabled",
        "exclude_patterns",
        "update_pr_description",
        "custom_instructions",
        "allow_auto_fix",
    ];
    if !ALLOWED_KEYS.contains(&key.as_str()) {
        return Err(ApiError::bad_request(format!("Unknown setting: {key}")));
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
) -> Result<Json<serde_json::Value>, ApiError> {
    for key in GITHUB_KEYS {
        crate::db::db_execute!(&state.pool, "DELETE FROM app_config WHERE key = ?", key)?;
    }
    Ok(Json(
        json!({ "status": "ok", "message": "GitHub App configuration removed" }),
    ))
}

async fn rotate_github_credentials(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    for key in GITHUB_KEYS {
        crate::db::db_execute!(&state.pool, "DELETE FROM app_config WHERE key = ?", key)?;
    }
    Ok(Json(
        json!({ "status": "ok", "message": "Credentials cleared. Re-run the manifest flow to set up a new GitHub App." }),
    ))
}
