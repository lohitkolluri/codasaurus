use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::AppState;

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(stats))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/stats
async fn stats(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    // Core stats are computed by the DB layer
    let mut core: serde_json::Value = db::reviews::get_stats(&state.pool).await?;

    // Recent activity: latest 10 reviews with repo full_name
    #[derive(sqlx::FromRow)]
    struct RecentReview {
        id: i64,
        repo_name: String,
        pr_number: i64,
        pr_title: Option<String>,
        status: String,
        created_at: String,
    }

    let recent_activity = sqlx::query_as::<_, RecentReview>(
        "SELECT r.id, COALESCE(repo.full_name, '') AS repo_name, r.pr_number, r.pr_title, r.status, r.created_at
         FROM reviews r
         LEFT JOIN repos repo ON repo.id = r.repo_id
         ORDER BY r.created_at DESC
         LIMIT 10",
    )
    .fetch_all(&state.pool.0)
    .await?;

    let activity: Vec<serde_json::Value> = recent_activity
        .into_iter()
        .map(|rev| {
            json!({
                "id": rev.id,
                "repo": rev.repo_name,
                "pr_number": rev.pr_number,
                "pr_title": rev.pr_title,
                "status": rev.status,
                "created_at": rev.created_at,
            })
        })
        .collect();

    if let Some(obj) = core.as_object_mut() {        obj.insert("recent_activity".into(), json!(activity));
    }
    Ok(Json(core))
}
