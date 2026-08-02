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
    pub page: Option<i64>,
    pub per_page: Option<i64>,
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

/// GET /api/audit
async fn list_audit(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(params): Query<ListAuditParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;
    let limit = params.per_page.or(params.limit).unwrap_or(50).clamp(1, 500);
    let offset = if let Some(page) = params.page {
        ((page.max(1) - 1) * limit).max(0)
    } else {
        params.offset.unwrap_or(0).max(0)
    };
    let event_type = params.event_type.as_deref();

    let entries = db::audit::list_audit_entries(&state.pool, event_type, limit, offset).await?;

    // Total count (matching filter)
    let total: i64 = match event_type {
        Some(et) => crate::db::db_scalar!(
            &state.pool,
            i64,
            "SELECT COUNT(*) FROM audit_log WHERE event_type = ?",
            et
        )?,
        None => crate::db::db_scalar!(&state.pool, i64, "SELECT COUNT(*) FROM audit_log")?,
    };
    let total_pages = ((total as f64) / (limit as f64)).ceil() as i64;

    Ok(Json(json!({
        "entries": entries,
        "total": total,
        "total_pages": total_pages.max(1),
        "limit": limit,
        "offset": offset,
        "retention_days": db::audit::retention_days(),
    })))
}
