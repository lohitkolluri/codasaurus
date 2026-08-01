use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::db;
use crate::db::DbPool;
use crate::learning::store::LearningStore;

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

#[derive(Deserialize)]
pub struct DismissBody {
    pub fingerprint: String,
    pub detector: Option<String>,
    pub file: Option<String>,
    pub message: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_reviews))
        .route("/{id}", get(get_review))
        .route("/{id}/findings", get(get_review_findings))
        .route("/dismiss", post(dismiss_finding))
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

    // Batch-load repo names (avoid N+1).
    let mut name_by_id: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
    let unique_ids: Vec<i64> = {
        let mut ids: Vec<i64> = reviews.iter().map(|r| r.repo_id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    if !unique_ids.is_empty() {
        let placeholders = std::iter::repeat_n("?", unique_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT id, full_name FROM repos WHERE id IN ({placeholders})");
        let prepared = state.pool.prepare_sql(&sql);
        let rows: Result<Vec<(i64, String)>, _> = match &state.pool {
            crate::db::DbPool::Sqlite(p) => {
                let mut q = sqlx::query_as::<_, (i64, String)>(&prepared);
                for id in &unique_ids {
                    q = q.bind(id);
                }
                q.fetch_all(p).await
            }
            crate::db::DbPool::Postgres(p) => {
                let mut q = sqlx::query_as::<_, (i64, String)>(&prepared);
                for id in &unique_ids {
                    q = q.bind(id);
                }
                q.fetch_all(p).await
            }
        };
        if let Ok(rows) = rows {
            for (id, name) in rows {
                name_by_id.insert(id, name);
            }
        }
    }

    let enriched: Vec<serde_json::Value> = reviews
        .iter()
        .map(|r| {
            let name = name_by_id.get(&r.repo_id).cloned().unwrap_or_default();
            let mut v = serde_json::to_value(r).unwrap_or_default();
            if let Some(obj) = v.as_object_mut() {
                obj.insert("repo_name".into(), json!(name.clone()));
                obj.insert("repo_full_name".into(), json!(name));
            }
            v
        })
        .collect();

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
        (Some(rid), Some(st)) => crate::db::db_scalar!(
            pool,
            i64,
            "SELECT COUNT(*) FROM reviews WHERE repo_id = ? AND status = ?",
            rid,
            st
        )?,
        (Some(rid), None) => crate::db::db_scalar!(
            pool,
            i64,
            "SELECT COUNT(*) FROM reviews WHERE repo_id = ?",
            rid
        )?,
        (None, Some(st)) => crate::db::db_scalar!(
            pool,
            i64,
            "SELECT COUNT(*) FROM reviews WHERE status = ?",
            st
        )?,
        (None, None) => crate::db::db_scalar!(pool, i64, "SELECT COUNT(*) FROM reviews")?,
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
        .ok_or_else(|| ApiError::not_found(format!("Review {id} not found")))?;

    let findings = db::reviews::get_findings_for_review(&state.pool, id).await?;

    let repo_full_name: Option<String> = crate::db::db_scalar_optional!(
        &state.pool,
        String,
        "SELECT full_name FROM repos WHERE id = ?",
        review.repo_id
    )
    .ok()
    .flatten();

    let mut review_val = serde_json::to_value(&review).unwrap_or_default();
    if let Some(obj) = review_val.as_object_mut() {
        obj.insert(
            "repo_full_name".into(),
            json!(repo_full_name.clone().unwrap_or_default()),
        );
    }

    Ok(Json(json!({
        "review": review_val,
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
        .ok_or_else(|| ApiError::not_found(format!("Review {id} not found")))?;

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

/// POST /api/reviews/dismiss — dismiss a finding into the learning store
async fn dismiss_finding(
    State(state): State<AppState>,
    Json(body): Json<DismissBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let fp = body.fingerprint.trim();
    if fp.len() < 8 {
        return Err(ApiError::bad_request("fingerprint too short"));
    }
    // Strip review_id: prefix if present from DB storage
    let fp = fp.rsplit(':').next().unwrap_or(fp);
    let store = LearningStore::from_pool(&state.pool);
    store
        .dismiss_fingerprint(
            fp,
            body.detector.as_deref().unwrap_or("manual"),
            body.file.as_deref().unwrap_or(""),
            body.message.as_deref().unwrap_or("dismissed via dashboard"),
        )
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "status": "ok", "fingerprint": fp })))
}
