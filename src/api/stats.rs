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

    let recent_activity = crate::db::db_fetch_all!(
        &state.pool,
        RecentReview,
        "SELECT r.id, COALESCE(repo.full_name, '') AS repo_name, r.pr_number, r.pr_title, r.status, r.created_at
         FROM reviews r
         LEFT JOIN repos repo ON repo.id = r.repo_id
         ORDER BY r.created_at DESC
         LIMIT 10"
    )?;

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

    if let Some(obj) = core.as_object_mut() {
        obj.insert("recent_activity".into(), json!(activity));
    }

    // Trust panel: FP proxy + accept rate from dismissals vs Tier-1 findings.
    crate::metrics::refresh_from_db(&state.pool).await;
    let dismissals = crate::db::db_scalar!(
        &state.pool,
        i64,
        "SELECT COUNT(*) FROM dismissed_findings"
    )
    .unwrap_or(0);
    let tier1 = crate::db::db_scalar!(
        &state.pool,
        i64,
        "SELECT COUNT(*) FROM findings WHERE detector IN ('secrets','vulnerabilities','iac','hallucinated-imports','phantom-deps')"
    )
    .unwrap_or(0);
    let total_findings = crate::db::db_scalar!(&state.pool, i64, "SELECT COUNT(*) FROM findings")
        .unwrap_or(0);
    let fp_proxy = if tier1 == 0 {
        0.0
    } else {
        dismissals as f64 / tier1 as f64
    };
    // Accept proxy: clamp dismissals so rate stays in [0, 100] even when tables diverge.
    let accept_rate = if total_findings == 0 {
        None
    } else {
        let dismissed_capped = dismissals.min(total_findings);
        Some(((total_findings - dismissed_capped) as f64 / total_findings as f64) * 100.0)
    };
    if let Some(obj) = core.as_object_mut() {
        obj.insert(
            "trust".into(),
            json!({
                "dismissals": dismissals,
                "tier1_findings": tier1,
                "fp_proxy_ratio": fp_proxy,
                "accept_rate": accept_rate,
                "note": "Proxy metrics: dismissals table vs current findings rows — not a lifetime cohort.",
            }),
        );
        obj.insert(
            "llm".into(),
            json!({
                "requests": crate::metrics::llm_request_count(),
                "prompt_chars": crate::metrics::llm_prompt_chars_total(),
                "spend_usd_estimate": crate::metrics::llm_spend_usd_estimate(),
                "spend_usd_last_day": crate::db::events::spend_usd_last_day(&state.pool).await,
                "daily_budget_usd": crate::llm::budget::daily_budget_usd(Some(&state.pool)).await,
                "note": "Estimates from agent_events + process metrics; not billing truth.",
            }),
        );
    }

    Ok(Json(core))
}
