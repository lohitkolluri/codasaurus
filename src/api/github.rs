use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/install-url", get(install_url))
        .route("/manage-url", get(manage_url))
}

async fn install_url(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let slug = db::config::get_config(&state.pool, "github_app_slug")
        .await
        .ok()
        .flatten();

    if let Some(slug) = slug {
        return Ok(Json(json!({
            "url": format!("https://github.com/apps/{}/installations/new", slug)
        })));
    }

    if std::env::var("GITHUB_APP_ID").is_ok() {
        return Ok(Json(json!({
            "url": "https://github.com/settings/installations",
        })));
    }

    Ok(Json(json!({
        "url": null,
        "error": "No GitHub App configured. Run the setup wizard first."
    })))
}

async fn manage_url(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let slug = db::config::get_config(&state.pool, "github_app_slug")
        .await
        .ok()
        .flatten();

    if let Some(slug) = slug {
        let install_id: Option<i64> = sqlx::query_scalar(
            "SELECT installation_id FROM repos WHERE installation_id IS NOT NULL LIMIT 1",
        )
        .fetch_optional(&state.pool.0)
        .await
        .ok()
        .flatten();

        let url = if let Some(iid) = install_id {
            format!("https://github.com/settings/installations/{}", iid)
        } else {
            format!("https://github.com/apps/{}/installations/new", slug)
        };

        return Ok(Json(json!({ "url": url })));
    }

    if std::env::var("GITHUB_APP_ID").is_ok() {
        return Ok(Json(json!({
            "url": "https://github.com/settings/installations",
        })));
    }

    Ok(Json(json!({
        "url": null,
        "error": "No GitHub App configured."
    })))
}
