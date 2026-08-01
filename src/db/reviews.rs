use crate::db::models::*;
use crate::db::{
    db_execute, db_fetch_all, db_fetch_one, db_fetch_optional, db_scalar, DbPool,
};

pub async fn list_reviews(
    pool: &DbPool,
    repo_id: Option<i64>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Review>, sqlx::Error> {
    match (repo_id, status) {
        (Some(rid), Some(st)) => db_fetch_all!(
            pool,
            Review,
            "SELECT * FROM reviews WHERE repo_id = ? AND status = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            rid,
            st,
            limit,
            offset
        ),
        (Some(rid), None) => db_fetch_all!(
            pool,
            Review,
            "SELECT * FROM reviews WHERE repo_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            rid,
            limit,
            offset
        ),
        (None, Some(st)) => db_fetch_all!(
            pool,
            Review,
            "SELECT * FROM reviews WHERE status = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            st,
            limit,
            offset
        ),
        (None, None) => db_fetch_all!(
            pool,
            Review,
            "SELECT * FROM reviews ORDER BY created_at DESC LIMIT ? OFFSET ?",
            limit,
            offset
        ),
    }
}

pub async fn get_review(pool: &DbPool, id: i64) -> Result<Option<Review>, sqlx::Error> {
    db_fetch_optional!(pool, Review, "SELECT * FROM reviews WHERE id = ?", id)
}

pub async fn create_review(pool: &DbPool, review: &ReviewCreate) -> Result<Review, sqlx::Error> {
    let sha = review.pr_head_sha.clone().unwrap_or_default();
    db_fetch_one!(
        pool,
        Review,
        "INSERT INTO reviews (repo_id, pr_number, pr_title, pr_author, pr_base_branch, pr_head_branch, pr_head_sha)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(repo_id, pr_number, pr_head_sha) DO UPDATE SET
           pr_title = excluded.pr_title,
           pr_author = excluded.pr_author,
           pr_base_branch = excluded.pr_base_branch,
           pr_head_branch = excluded.pr_head_branch,
           status = 'pending',
           started_at = NULL,
           completed_at = NULL,
           summary_json = NULL
         RETURNING *",
        review.repo_id,
        review.pr_number,
        &review.pr_title,
        &review.pr_author,
        &review.pr_base_branch,
        &review.pr_head_branch,
        &sha
    )
}

pub async fn delete_findings_for_review(
    pool: &DbPool,
    review_id: i64,
) -> Result<(), sqlx::Error> {
    db_execute!(pool, "DELETE FROM findings WHERE review_id = ?", review_id)?;
    Ok(())
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
            db_execute!(
                pool,
                "UPDATE reviews SET status = ?, summary_json = ?, completed_at = ? WHERE id = ?",
                status,
                sj,
                ca,
                id
            )?;
        }
        (Some(status), Some(sj), None) => {
            db_execute!(
                pool,
                "UPDATE reviews SET status = ?, summary_json = ? WHERE id = ?",
                status,
                sj,
                id
            )?;
        }
        (Some(status), None, Some(ca)) => {
            db_execute!(
                pool,
                "UPDATE reviews SET status = ?, completed_at = ? WHERE id = ?",
                status,
                ca,
                id
            )?;
        }
        (Some(status), None, None) => {
            db_execute!(pool, "UPDATE reviews SET status = ? WHERE id = ?", status, id)?;
        }
        (None, Some(sj), Some(ca)) => {
            db_execute!(
                pool,
                "UPDATE reviews SET summary_json = ?, completed_at = ? WHERE id = ?",
                sj,
                ca,
                id
            )?;
        }
        (None, Some(sj), None) => {
            db_execute!(
                pool,
                "UPDATE reviews SET summary_json = ? WHERE id = ?",
                sj,
                id
            )?;
        }
        (None, None, Some(ca)) => {
            db_execute!(
                pool,
                "UPDATE reviews SET completed_at = ? WHERE id = ?",
                ca,
                id
            )?;
        }
        (None, None, None) => {}
    }
    Ok(())
}

pub async fn get_findings_for_review(
    pool: &DbPool,
    review_id: i64,
) -> Result<Vec<Finding>, sqlx::Error> {
    db_fetch_all!(
        pool,
        Finding,
        "SELECT * FROM findings WHERE review_id = ? ORDER BY severity, file_path",
        review_id
    )
}

pub async fn create_findings_batch(
    pool: &DbPool,
    findings: &[FindingCreate],
) -> Result<(), sqlx::Error> {
    if findings.is_empty() {
        return Ok(());
    }
    let sql = "INSERT INTO findings (review_id, fingerprint, file_path, line_start, line_end, column_start, column_end, severity, detector, rule_id, message, suggested_fix, code_snippet, context, category)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
    match pool {
        DbPool::Sqlite(p) => {
            let mut tx = p.begin().await?;
            for finding in findings {
                sqlx::query(sql)
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
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
        }
        DbPool::Postgres(p) => {
            let sql = pool.prepare_sql(sql);
            let mut tx = p.begin().await?;
            for finding in findings {
                sqlx::query(&sql)
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
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
        }
    }
    Ok(())
}

pub async fn get_stats(pool: &DbPool) -> Result<serde_json::Value, sqlx::Error> {
    let total_repos: i64 = db_scalar!(
        pool,
        i64,
        "SELECT COUNT(*) FROM repos WHERE active = ?",
        true
    )?;

    let total_reviews_today: i64 = if pool.is_postgres() {
        db_scalar!(
            pool,
            i64,
            "SELECT COUNT(*) FROM reviews WHERE created_at::date = CURRENT_DATE"
        )?
    } else {
        db_scalar!(
            pool,
            i64,
            "SELECT COUNT(*) FROM reviews
             WHERE created_at >= date('now') AND created_at < date('now', '+1 day')"
        )?
    };

    let pass_rate: Option<f64> = db_scalar!(
        pool,
        Option<f64>,
        "SELECT AVG(CASE WHEN status = 'passed' THEN 100.0 WHEN status = 'failed' THEN 0.0 ELSE NULL END) FROM reviews"
    )?;

    let total_findings: i64 = db_scalar!(pool, i64, "SELECT COUNT(*) FROM findings")?;

    Ok(serde_json::json!({
        "total_repos": total_repos,
        "total_reviews_today": total_reviews_today,
        "pass_rate": pass_rate,
        "total_findings": total_findings,
    }))
}
