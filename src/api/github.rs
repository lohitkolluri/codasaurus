use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/install-url", get(install_url))
        .route("/manage-url", get(manage_url))
        .route("/resolve", post(resolve_slug))
}

async fn resolve_app_slug(state: &AppState) -> Option<String> {
    let from_db = db::config::get_config(&state.pool, "github_app_slug")
        .await
        .ok()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if from_db.is_some() {
        return from_db;
    }
    std::env::var("GITHUB_APP_SLUG")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn install_url(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(slug) = ensure_slug(&state).await {
        return Ok(Json(json!({
            "url": format!("https://github.com/apps/{}/installations/new", slug)
        })));
    }

    if std::env::var("GITHUB_APP_ID").is_ok() {
        return Ok(Json(json!({
            "url": null,
            "error": "GitHub App slug is unknown, so no direct install link exists. Open https://github.com/settings/apps, pick your app, and install it from there — or set GITHUB_APP_SLUG to the app slug."
        })));
    }

    Ok(Json(json!({
        "url": null,
        "error": "No GitHub App configured. Run the setup wizard first."
    })))
}

async fn manage_url(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(slug) = ensure_slug(&state).await {
        let install_id: Option<i64> = crate::db::db_scalar_optional!(
            &state.pool,
            i64,
            "SELECT installation_id FROM repos WHERE installation_id IS NOT NULL LIMIT 1"
        )
        .ok()
        .flatten();

        let url = if let Some(iid) = install_id {
            format!("https://github.com/settings/installations/{iid}")
        } else {
            format!("https://github.com/apps/{slug}/installations/new")
        };

        return Ok(Json(json!({ "url": url })));
    }

    if std::env::var("GITHUB_APP_ID").is_ok() {
        return Ok(Json(json!({
            "url": null,
            "error": "GitHub App slug is unknown, so no direct install link exists. Open https://github.com/settings/apps, pick your app, and install it from there — or set GITHUB_APP_SLUG to the app slug."
        })));
    }

    Ok(Json(json!({
        "url": null,
        "error": "No GitHub App configured."
    })))
}

/// Slug with a live fallback: when the slug was never stored (manual or env
/// setup), ask GitHub `GET /app` with our own App JWT and persist what it
/// returns — so the dashboard can build install links with zero user action.
async fn ensure_slug(state: &AppState) -> Option<String> {
    if let Some(slug) = resolve_app_slug(state).await {
        return Some(slug);
    }
    resolve_slug_live(state).await
}

/// Query `GET /app` with the configured App credentials and store the
/// returned slug/name. Returns the slug on success.
async fn resolve_slug_live(state: &AppState) -> Option<String> {
    let app_id = db::config::get_config(&state.pool, "github_app_id")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("GITHUB_APP_ID").ok())?;
    let private_key = db::config::get_config(&state.pool, "github_private_key")
        .await
        .ok()
        .flatten()
        .filter(|k| !k.trim().is_empty())
        .or_else(crate::github_jwt::resolve_private_key_from_env)?;
    let token = crate::github_jwt::create_app_jwt(&app_id, &private_key).ok()?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let info: serde_json::Value = client
        .get("https://api.github.com/app")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "codasaurus")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let slug = info.get("slug").and_then(|v| v.as_str())?.to_string();
    if slug.trim().is_empty() {
        return None;
    }
    let _ = db::config::set_config(&state.pool, "github_app_slug", &slug).await;
    db::config::apply_setting_to_env("github_app_slug", &slug);
    if let Some(name) = info.get("name").and_then(|v| v.as_str()) {
        let _ = db::config::set_config(&state.pool, "github_app_name", name).await;
    }
    Some(slug)
}

/// POST /api/github/resolve — re-resolve slug/name from GitHub and persist.
/// Powers the dashboard "Repair install link" button.
async fn resolve_slug(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;
    match resolve_slug_live(&state).await {
        Some(slug) => Ok(Json(json!({
            "status": "ok",
            "slug": slug,
            "url": format!("https://github.com/apps/{slug}/installations/new"),
        }))),
        None => Err(ApiError::bad_request(
            "Could not resolve the GitHub App from stored credentials. Check the App ID and private key, then try again.",
        )),
    }
}
