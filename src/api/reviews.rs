use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::db;
use crate::db::DbPool;

use super::errors::ApiError;
use super::AppState;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct ListReviewsParams {
    pub repo_id: Option<i64>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_reviews))
        .route("/{id}", get(get_review))
        .route("/{id}/findings", get(get_review_findings))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/reviews
async fn list_reviews(
    State(state): State<AppState>,
    Query(params): Query<ListReviewsParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let limit = params.limit.unwrap_or(20).clamp(1, 200);
    let offset = params.offset.unwrap_or(0).max(0);
    let repo_id = params.repo_id;
    let status = params.status.as_deref();

    let reviews = db::reviews::list_reviews(&state.pool, repo_id, status, limit, offset).await?;

    // Total count for pagination (match the same filters)
    let total: i64 = count_reviews(&state.pool, repo_id, status).await?;

    Ok(Json(json!({
        "reviews": reviews,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

/// Helper: COUNT query matching list_reviews filters.
async fn count_reviews(
    pool: &DbPool,
    repo_id: Option<i64>,
    status: Option<&str>,
) -> Result<i64, ApiError> {
    let count = match (repo_id, status) {
        (Some(rid), Some(st)) => {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM reviews WHERE repo_id = ? AND status = ?",
            )
            .bind(rid)
            .bind(st)
            .fetch_one(&pool.0)
            .await?
        }
        (Some(rid), None) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM reviews WHERE repo_id = ?")
                .bind(rid)
                .fetch_one(&pool.0)
                .await?
        }
        (None, Some(st)) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM reviews WHERE status = ?")
                .bind(st)
                .fetch_one(&pool.0)
                .await?
        }
        (None, None) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM reviews")
                .fetch_one(&pool.0)
                .await?
        }
    };
    Ok(count)
}

/// GET /api/v1/reviews/:id
async fn get_review(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let review = db::reviews::get_review(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Review {} not found", id)))?;

    let findings = db::reviews::get_findings_for_review(&state.pool, id).await?;

    Ok(Json(json!({
        "review": review,
        "findings": findings,
    })))
}

/// GET /api/v1/reviews/:id/findings
async fn get_review_findings(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Verify the review exists first
    let _review = db::reviews::get_review(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Review {} not found", id)))?;

    let findings = db::reviews::get_findings_for_review(&state.pool, id).await?;

    // Group findings by file_path
    let mut grouped: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
        std::collections::BTreeMap::new();
    for f in findings {
        let entry = grouped.entry(f.file_path.clone()).or_default();
        entry.push(json!(f));
    }

    Ok(Json(json!({
        "review_id": id,
        "findings_by_file": grouped,
        "total_findings": grouped.values().map(|v| v.len()).sum::<usize>(),
    })))
}
