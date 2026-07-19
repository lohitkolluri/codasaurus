use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/install-url", get(install_url))
}

async fn install_url(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let slug = db::config::get_config(&state.pool, "github_app_slug")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    if slug.is_empty() {
        return Ok(Json(json!({
            "url": null,
            "error": "No GitHub App configured. Run the setup wizard first."
        })));
    }

    Ok(Json(json!({
        "url": format!("https://github.com/apps/{}/installations/new", slug)
    })))
}
