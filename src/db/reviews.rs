use crate::db::models::*;
use crate::db::DbPool;

pub async fn list_reviews(
    pool: &DbPool,
    repo_id: Option<i64>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Review>, sqlx::Error> {
    match (repo_id, status) {
        (Some(rid), Some(st)) => {
            sqlx::query_as::<_, Review>(
                "SELECT * FROM reviews WHERE repo_id = ? AND status = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(rid)
            .bind(st)
            .bind(limit)
            .bind(offset)
            .fetch_all(&pool.0)
            .await
        }
        (Some(rid), None) => {
            sqlx::query_as::<_, Review>(
                "SELECT * FROM reviews WHERE repo_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(rid)
            .bind(limit)
            .bind(offset)
            .fetch_all(&pool.0)
            .await
        }
        (None, Some(st)) => {
            sqlx::query_as::<_, Review>(
                "SELECT * FROM reviews WHERE status = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(st)
            .bind(limit)
            .bind(offset)
            .fetch_all(&pool.0)
            .await
        }
        (None, None) => {
            sqlx::query_as::<_, Review>(
                "SELECT * FROM reviews ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&pool.0)
            .await
        }
    }
}

pub async fn get_review(pool: &DbPool, id: i64) -> Result<Option<Review>, sqlx::Error> {
    sqlx::query_as::<_, Review>("SELECT * FROM reviews WHERE id = ?")
        .bind(id)
        .fetch_optional(&pool.0)
        .await
}

pub async fn create_review(pool: &DbPool, review: &ReviewCreate) -> Result<Review, sqlx::Error> {
    sqlx::query_as::<_, Review>(
        "INSERT INTO reviews (repo_id, pr_number, pr_title, pr_author, pr_base_branch, pr_head_branch, pr_head_sha)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING *",
    )
    .bind(review.repo_id)
    .bind(review.pr_number)
    .bind(&review.pr_title)
    .bind(&review.pr_author)
    .bind(&review.pr_base_branch)
    .bind(&review.pr_head_branch)
    .bind(&review.pr_head_sha)
    .fetch_one(&pool.0)
    .await
}

pub async fn update_review(
    pool: &DbPool,
    id: i64,
    update: &ReviewUpdate,
) -> Result<(), sqlx::Error> {
    match (
        update.status.as_deref(),
        update.summary_json.as_deref(),
        update.completed_at.as_deref(),
    ) {
        (Some(status), Some(sj), Some(ca)) => {
            sqlx::query(
                "UPDATE reviews SET status = ?, summary_json = ?, completed_at = ? WHERE id = ?",
            )
            .bind(status)
            .bind(sj)
            .bind(ca)
            .bind(id)
            .execute(&pool.0)
            .await?;
        }
        (Some(status), Some(sj), None) => {
            sqlx::query("UPDATE reviews SET status = ?, summary_json = ? WHERE id = ?")
                .bind(status)
                .bind(sj)
                .bind(id)
                .execute(&pool.0)
                .await?;
        }
        (Some(status), None, Some(ca)) => {
            sqlx::query("UPDATE reviews SET status = ?, completed_at = ? WHERE id = ?")
                .bind(status)
                .bind(ca)
                .bind(id)
                .execute(&pool.0)
                .await?;
        }
        (Some(status), None, None) => {
            sqlx::query("UPDATE reviews SET status = ? WHERE id = ?")
                .bind(status)
                .bind(id)
                .execute(&pool.0)
                .await?;
        }
        (None, Some(sj), Some(ca)) => {
            sqlx::query("UPDATE reviews SET summary_json = ?, completed_at = ? WHERE id = ?")
                .bind(sj)
                .bind(ca)
                .bind(id)
                .execute(&pool.0)
                .await?;
        }
        (None, Some(sj), None) => {
            sqlx::query("UPDATE reviews SET summary_json = ? WHERE id = ?")
                .bind(sj)
                .bind(id)
                .execute(&pool.0)
                .await?;
        }
        (None, None, Some(ca)) => {
            sqlx::query("UPDATE reviews SET completed_at = ? WHERE id = ?")
                .bind(ca)
                .bind(id)
                .execute(&pool.0)
                .await?;
        }
        (None, None, None) => {
            // Nothing to update
        }
    }
    Ok(())
}

pub async fn get_findings_for_review(
    pool: &DbPool,
    review_id: i64,
) -> Result<Vec<Finding>, sqlx::Error> {
    sqlx::query_as::<_, Finding>(
        "SELECT * FROM findings WHERE review_id = ? ORDER BY severity, file_path",
    )
    .bind(review_id)
    .fetch_all(&pool.0)
    .await
}

pub async fn create_finding(
    pool: &DbPool,
    finding: &FindingCreate,
) -> Result<Finding, sqlx::Error> {
    sqlx::query_as::<_, Finding>(
        "INSERT INTO findings (review_id, fingerprint, file_path, line_start, line_end, column_start, column_end, severity, detector, rule_id, message, suggested_fix, code_snippet, context, category)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING *",
    )
    .bind(finding.review_id)
    .bind(&finding.fingerprint)
    .bind(&finding.file_path)
    .bind(finding.line_start)
    .bind(finding.line_end)
    .bind(finding.column_start)
    .bind(finding.column_end)
    .bind(&finding.severity)
    .bind(&finding.detector)
    .bind(&finding.rule_id)
    .bind(&finding.message)
    .bind(&finding.suggested_fix)
    .bind(&finding.code_snippet)
    .bind(&finding.context)
    .bind(&finding.category)
    .fetch_one(&pool.0)
    .await
}

pub async fn get_stats(pool: &DbPool) -> Result<serde_json::Value, sqlx::Error> {
    let total_repos: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM repos WHERE active = 1")
        .fetch_one(&pool.0)
        .await?;

    let total_reviews_today: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM reviews WHERE date(created_at) = CURRENT_DATE")
            .fetch_one(&pool.0)
            .await?;

    let pass_rate: Option<f64> = sqlx::query_scalar(
        "SELECT AVG(CASE WHEN status = 'passed' THEN 100.0 WHEN status = 'failed' THEN 0.0 ELSE NULL END) FROM reviews",
    )
    .fetch_one(&pool.0)
    .await?;

    let total_findings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM findings")
        .fetch_one(&pool.0)
        .await?;

    Ok(serde_json::json!({
        "total_repos": total_repos,
        "total_reviews_today": total_reviews_today,
        "pass_rate": pass_rate,
        "total_findings": total_findings,
    }))
}
