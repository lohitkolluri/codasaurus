use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::AppState;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct ListAuditParams {
    pub event_type: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_audit))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/audit
async fn list_audit(
    State(state): State<AppState>,
    Query(params): Query<ListAuditParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let offset = params.offset.unwrap_or(0).max(0);
    let event_type = params.event_type.as_deref();

    let entries = db::audit::list_audit_entries(&state.pool, event_type, limit, offset).await?;

    // Total count (matching filter)
    let total: i64 = match event_type {
        Some(et) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE event_type = ?")
                .bind(et)
                .fetch_one(&state.pool.0)
                .await?
        }
        None => {
            sqlx::query_scalar("SELECT COUNT(*) FROM audit_log")
                .fetch_one(&state.pool.0)
                .await?
        }
    };

    Ok(Json(json!({
        "entries": entries,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}
