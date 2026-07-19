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
        .flatten();

    if let Some(slug) = slug {
        return Ok(Json(json!({
            "url": format!("https://github.com/apps/{}/installations/new", slug)
        })));
    }

    // If configured via env vars but slug isn't stored, fall back to
    // the app install page (user can find their app there).
    if std::env::var("GITHUB_APP_ID").is_ok() {
        return Ok(Json(json!({
            "url": "https://github.com/settings/installations",
            "note": "Your app was configured via environment variables. Visit GitHub to manage installations."
        })));
    }

    Ok(Json(json!({
        "url": null,
        "error": "No GitHub App configured. Run the setup wizard first."
    })))
}
