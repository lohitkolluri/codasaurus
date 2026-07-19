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
pub struct UpdateRepoBody {
    pub config_json: String,
    pub active: bool,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_repos))
        .route("/{id}", get(get_repo))
        .route("/{id}", put(update_repo))
        .route("/{id}", delete(delete_repo))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/repos
async fn list_repos(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let repos = db::repos::list_repos(&state.pool).await?;
    Ok(Json(json!(repos)))
}

/// GET /api/v1/repos/:id
async fn get_repo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let repo = db::repos::get_repo(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Repo {} not found", id)))?;
    Ok(Json(json!(repo)))
}

/// PUT /api/v1/repos/:id
async fn update_repo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateRepoBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    db::repos::update_repo(&state.pool, id, &body.config_json, body.active).await?;
    Ok(Json(json!({ "status": "ok" })))
}

/// DELETE /api/v1/repos/:id
async fn delete_repo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    db::repos::delete_repo(&state.pool, id).await?;
    Ok(Json(json!({ "status": "ok" })))
}
