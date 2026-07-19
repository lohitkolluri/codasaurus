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
    #[serde(alias = "page")]
    pub _page: Option<i64>,
    #[serde(alias = "per_page")]
    pub _per_page: Option<i64>,
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
    let total: i64 = count_reviews(&state.pool, repo_id, status).await?;
    let total_pages = ((total as f64) / (limit as f64)).ceil() as i64;

    // Enrich each review with its repo name
    let mut enriched: Vec<serde_json::Value> = Vec::new();
    for r in &reviews {
        let repo_name: Option<String> =
            sqlx::query_scalar("SELECT full_name FROM repos WHERE id = ?")
                .bind(r.repo_id)
                .fetch_optional(&state.pool.0)
                .await
                .ok()
                .flatten();
        let name = repo_name.clone().unwrap_or_default();
        let mut v = serde_json::to_value(r).unwrap_or_default();
        if let Some(obj) = v.as_object_mut() {
            obj.insert("repo_name".into(), json!(name));
            obj.insert("repo_full_name".into(), json!(name));
        }
        enriched.push(v);
    }

    Ok(Json(json!({
        "reviews": enriched,
        "total": total,
        "total_pages": total_pages,
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
            sqlx::query_scalar("SELECT COUNT(*) FROM reviews WHERE repo_id = ? AND status = ?")
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
