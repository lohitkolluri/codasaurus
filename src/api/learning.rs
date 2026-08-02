//! Learning rules admin API.

use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::learning::store::LearningStore;

use super::errors::ApiError;
use super::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/rules", get(list_rules))
        .route("/rules/{id}", delete(delete_rule))
        .route("/rules/{id}/approve", post(approve_rule))
        .route("/rules/{id}/archive", post(archive_rule))
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
                "status": r.status,
                "source_count": r.source_count,
                "repo_full_name": r.repo_full_name,
                "created_at": r.created_at,
            })
        })
        .collect();
    Ok(Json(json!({ "rules": items })))
}

async fn approve_rule(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;
    let store = LearningStore::from_pool(&state.pool);
    let updated = store
        .approve_rule(&id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if !updated {
        return Err(ApiError::not_found("rule not found or not suggested"));
    }
    Ok(Json(json!({ "status": "ok", "id": id })))
}

async fn archive_rule(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;
    let store = LearningStore::from_pool(&state.pool);
    let updated = store
        .archive_rule(&id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if !updated {
        return Err(ApiError::not_found("rule not found"));
    }
    Ok(Json(json!({ "status": "ok", "id": id })))
}

async fn delete_rule(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;
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
