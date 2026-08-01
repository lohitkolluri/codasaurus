//! Learning rules admin API.

use axum::extract::{Path, State};
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde_json::json;

use crate::learning::store::LearningStore;

use super::errors::ApiError;
use super::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/rules", get(list_rules))
        .route("/rules/{id}", delete(delete_rule))
}

async fn list_rules(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let store = LearningStore::from_pool(&state.pool);
    let rules = store
        .list_rules()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let items: Vec<serde_json::Value> = rules
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "detector": r.detector,
                "file_pattern": r.file_pattern,
                "message_pattern": r.message_pattern,
                "action": r.action.as_str(),
                "reason": r.reason,
                "created_at": r.created_at,
            })
        })
        .collect();
    Ok(Json(json!({ "rules": items })))
}

async fn delete_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = LearningStore::from_pool(&state.pool);
    let deleted = store
        .delete_rule(&id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if !deleted {
        return Err(ApiError::not_found("rule not found"));
    }
    Ok(Json(json!({ "status": "ok", "id": id })))
}
