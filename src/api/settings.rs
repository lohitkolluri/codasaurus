use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::db;
use crate::github_jwt;

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

const OIDC_KEYS: &[&str] = &[
    "oidc_issuer",
    "oidc_client_id",
    "oidc_client_secret",
    "oidc_redirect_uri",
    "oidc_scopes",
    "oidc_allow_open_join",
    "oidc_allow_unverified_email",
    "oidc_allow_public_client",
];

const TICKET_KEYS: &[&str] = &[
    "jira_base_url",
    "jira_email",
    "jira_api_token",
    "linear_api_key",
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
    "auto_approve",
    "pr_title_fix",
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
        .route("/github", get(get_github_settings))
        .route("/github", delete(delete_github_settings))
        .route("/github/test", post(test_github_connection))
        .route("/oidc", delete(delete_oidc_settings))
        .route("/oidc/test", post(test_oidc_connection))
        .route("/tickets", delete(delete_ticket_settings))
        .route("/jira/test", post(test_jira_connection))
        .route("/linear/test", post(test_linear_connection))
        // `/{key}` last so static segments win.
        .route("/{key}", put(set_setting))
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

/// GET /api/settings
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

    // Offline is a kill-switch independent of OpenRouter keys. Surface the
    // effective value when `CODASAURUS_OFFLINE` in the process env overrides DB.
    let db_off = map
        .get("offline_mode")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let (effective, source) = crate::bot::offline::offline_mode_source(db_off.as_deref());
    map.insert("offline_mode_effective".into(), json!(effective));
    map.insert("offline_mode_source".into(), json!(source));
    // Prefer DB when the kill-switch is on in app_config; process env alone can
    // look like a platform setting the operator never chose after `apply_db_to_env`.
    if effective {
        map.insert(
            "offline_mode_hint".into(),
            json!(
                "Offline mode is on in app_config (Settings → System). It is independent of your LLM API key — turn the toggle off and Save."
            ),
        );
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

/// PUT /api/settings/:key
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
    // Empty `metrics_token` clears the token; other secrets keep prior values when blank.
    if SENSITIVE_KEYS.contains(&key.as_str()) {
        if body.value.contains('•') || body.value.contains('*') {
            return Ok(Json(
                json!({ "status": "ok", "key": key, "skipped": "unchanged" }),
            ));
        }
        if body.value.is_empty() {
            if key == "metrics_token" {
                db::config::delete_config(&state.pool, &key).await?;
                db::config::apply_setting_to_env(&key, "");
                db::audit::log_event(
                    &state.pool,
                    "settings.updated",
                    Some(&actor.email),
                    Some("setting"),
                    None,
                )
                .await;
                return Ok(Json(json!({ "status": "ok", "key": key, "cleared": true })));
            }
            return Ok(Json(
                json!({ "status": "ok", "key": key, "skipped": "unchanged" }),
            ));
        }
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
        "public_url" | "oidc_redirect_uri" if !body.value.trim().is_empty() => {
            let v = body.value.trim();
            if !(v.starts_with("http://") || v.starts_with("https://")) {
                return Err(ApiError::bad_request(format!(
                    "{key} must be an http(s) URL"
                )));
            }
        }
        "oidc_issuer" | "jira_base_url" if !body.value.trim().is_empty() => {
            crate::ssrf::validate_http_url_resolved(body.value.trim(), false)
                .await
                .map_err(ApiError::bad_request)?;
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
        // Clearing the kill-switch: drop process env so a stale CODASAURUS_OFFLINE=1
        // from an earlier mirror does not keep LLM disabled until restart.
        // Platform dashboard env vars still win on the next deploy — remove them there.
        if !on {
            std::env::remove_var("CODASAURUS_OFFLINE");
        }
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

async fn clear_mirrored_keys(pool: &db::DbPool, keys: &[&str]) -> Result<(), ApiError> {
    for key in keys {
        db::config::delete_config(pool, key).await?;
        db::config::apply_setting_to_env(key, "");
    }
    Ok(())
}

async fn config_or_env(pool: &db::DbPool, db_key: &str, env_key: &str) -> Option<String> {
    if let Ok(Some(v)) = db::config::get_config(pool, db_key).await {
        if !v.is_empty() {
            return Some(v);
        }
    }
    std::env::var(env_key).ok().filter(|v| !v.is_empty())
}

fn http_client(secs: u64) -> Result<reqwest::Client, ApiError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(secs))
        .build()
        .map_err(|e| ApiError::internal(format!("HTTP client: {e}")))
}

/// POST /api/settings/github/test — verify App JWT against GET /app.
async fn test_github_connection(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;

    let cfg = crate::bot::current_bot_config();
    let (app_id, private_key) = if let Some(c) = cfg {
        (c.app_id, c.private_key)
    } else {
        let app_id = config_or_env(&state.pool, "github_app_id", "GITHUB_APP_ID")
            .await
            .ok_or_else(|| ApiError::bad_request("GitHub App is not configured"))?;
        let private_key = db::config::get_config(&state.pool, "github_private_key")
            .await
            .ok()
            .flatten()
            .filter(|k| !k.trim().is_empty())
            .or_else(crate::github_jwt::resolve_private_key_from_env)
            .ok_or_else(|| ApiError::bad_request("GitHub App private key is not configured"))?;
        (app_id, private_key)
    };

    let token = github_jwt::create_app_jwt(&app_id, &private_key)
        .map_err(|e| ApiError::bad_request(format!("Invalid GitHub App credentials: {e}")))?;

    let client = http_client(10)?;
    let resp = client
        .get("https://api.github.com/app")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "codasaurus")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| ApiError::bad_request(format!("GitHub API request failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(ApiError::bad_request(format!(
            "GitHub rejected App credentials (HTTP {})",
            resp.status()
        )));
    }

    let body: serde_json::Value = resp.json().await.unwrap_or(json!({}));
    let name = body["name"].as_str().unwrap_or("GitHub App");
    Ok(Json(json!({
        "status": "ok",
        "ok": true,
        "message": format!("Connected as {name}"),
        "app_id": app_id,
        "app_name": name,
    })))
}

/// DELETE /api/settings/oidc — clear SSO config from DB + process env.
async fn delete_oidc_settings(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = super::rbac::require_owner(&state, &headers).await?;
    clear_mirrored_keys(&state.pool, OIDC_KEYS).await?;
    db::audit::log_event(
        &state.pool,
        "oidc.config_cleared",
        Some(&actor.email),
        Some("oidc"),
        None,
    )
    .await;
    Ok(Json(json!({
        "status": "ok",
        "message": "SSO (OIDC) configuration cleared.",
    })))
}

/// POST /api/settings/oidc/test — fetch OpenID discovery document.
async fn test_oidc_connection(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;

    let issuer = config_or_env(&state.pool, "oidc_issuer", "OIDC_ISSUER")
        .await
        .ok_or_else(|| ApiError::bad_request("OIDC issuer is not configured"))?;
    let issuer = issuer.trim().trim_end_matches('/').to_string();
    crate::ssrf::validate_http_url_resolved(&issuer, false)
        .await
        .map_err(ApiError::bad_request)?;

    let discovery_url = format!("{issuer}/.well-known/openid-configuration");
    let client = http_client(10)?;
    let resp = client
        .get(&discovery_url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ApiError::bad_request(format!("OIDC discovery request failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(ApiError::bad_request(format!(
            "OIDC discovery failed (HTTP {})",
            resp.status()
        )));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ApiError::bad_request(format!("Invalid OIDC discovery JSON: {e}")))?;

    let auth = body["authorization_endpoint"].as_str().unwrap_or("");
    let token = body["token_endpoint"].as_str().unwrap_or("");
    if auth.is_empty() || token.is_empty() {
        return Err(ApiError::bad_request(
            "OIDC discovery missing authorization_endpoint or token_endpoint",
        ));
    }

    Ok(Json(json!({
        "status": "ok",
        "ok": true,
        "message": "OIDC discovery succeeded",
        "issuer": issuer,
        "authorization_endpoint": auth,
        "token_endpoint": token,
    })))
}

/// DELETE /api/settings/tickets — clear Jira + Linear config.
async fn delete_ticket_settings(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = super::rbac::require_owner(&state, &headers).await?;
    clear_mirrored_keys(&state.pool, TICKET_KEYS).await?;
    db::audit::log_event(
        &state.pool,
        "tickets.config_cleared",
        Some(&actor.email),
        Some("tickets"),
        None,
    )
    .await;
    Ok(Json(json!({
        "status": "ok",
        "message": "Jira and Linear configuration cleared.",
    })))
}

/// POST /api/settings/jira/test — GET /rest/api/3/myself.
async fn test_jira_connection(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;

    let base = config_or_env(&state.pool, "jira_base_url", "JIRA_BASE_URL")
        .await
        .ok_or_else(|| ApiError::bad_request("Jira base URL is not configured"))?;
    let email = config_or_env(&state.pool, "jira_email", "JIRA_EMAIL")
        .await
        .ok_or_else(|| ApiError::bad_request("Jira email is not configured"))?;
    let token = config_or_env(&state.pool, "jira_api_token", "JIRA_API_TOKEN")
        .await
        .ok_or_else(|| ApiError::bad_request("Jira API token is not configured"))?;

    let base = base.trim().trim_end_matches('/');
    crate::ssrf::validate_http_url_resolved(base, false)
        .await
        .map_err(ApiError::bad_request)?;

    let url = format!("{base}/rest/api/3/myself");
    let client = http_client(10)?;
    let resp = client
        .get(&url)
        .basic_auth(&email, Some(&token))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ApiError::bad_request(format!("Jira request failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(ApiError::bad_request(format!(
            "Jira rejected credentials (HTTP {})",
            resp.status()
        )));
    }

    let body: serde_json::Value = resp.json().await.unwrap_or(json!({}));
    let display = body["displayName"]
        .as_str()
        .or_else(|| body["emailAddress"].as_str())
        .unwrap_or("Jira user");

    Ok(Json(json!({
        "status": "ok",
        "ok": true,
        "message": format!("Connected as {display}"),
    })))
}

/// POST /api/settings/linear/test — GraphQL viewer query.
async fn test_linear_connection(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;

    let api_key = config_or_env(&state.pool, "linear_api_key", "LINEAR_API_KEY")
        .await
        .ok_or_else(|| ApiError::bad_request("Linear API key is not configured"))?;

    let client = http_client(10)?;
    let query = json!({ "query": "{ viewer { id name email } }" });
    let resp = client
        .post("https://api.linear.app/graphql")
        .header("Authorization", api_key.as_str())
        .header("Content-Type", "application/json")
        .json(&query)
        .send()
        .await
        .map_err(|e| ApiError::bad_request(format!("Linear request failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(ApiError::bad_request(format!(
            "Linear rejected credentials (HTTP {})",
            resp.status()
        )));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ApiError::bad_request(format!("Invalid Linear response: {e}")))?;

    if body.get("errors").is_some() {
        let msg = body["errors"][0]["message"]
            .as_str()
            .unwrap_or("GraphQL error");
        return Err(ApiError::bad_request(format!("Linear API error: {msg}")));
    }

    let viewer = &body["data"]["viewer"];
    if viewer.is_null() {
        return Err(ApiError::bad_request(
            "Linear API returned no viewer (check API key)",
        ));
    }
    let name = viewer["name"]
        .as_str()
        .or_else(|| viewer["email"].as_str())
        .unwrap_or("Linear user");

    Ok(Json(json!({
        "status": "ok",
        "ok": true,
        "message": format!("Connected as {name}"),
    })))
}
