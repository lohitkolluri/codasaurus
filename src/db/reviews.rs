use crate::db::models::*;
use crate::db::{db_execute, db_fetch_all, db_fetch_one, db_fetch_optional, db_scalar, DbPool};

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

pub async fn delete_findings_for_review(pool: &DbPool, review_id: i64) -> Result<(), sqlx::Error> {
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
        update.completed_at,
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
            db_execute!(
                pool,
                "UPDATE reviews SET status = ? WHERE id = ?",
                status,
                id
            )?;
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

    // Single multi-row INSERT via UNNEST — one round-trip per review.
    let mut review_ids = Vec::with_capacity(findings.len());
    let mut fingerprints: Vec<Option<String>> = Vec::with_capacity(findings.len());
    let mut file_paths = Vec::with_capacity(findings.len());
    let mut line_starts: Vec<Option<i64>> = Vec::with_capacity(findings.len());
    let mut line_ends: Vec<Option<i64>> = Vec::with_capacity(findings.len());
    let mut column_starts: Vec<Option<i64>> = Vec::with_capacity(findings.len());
    let mut column_ends: Vec<Option<i64>> = Vec::with_capacity(findings.len());
    let mut severities = Vec::with_capacity(findings.len());
    let mut detectors = Vec::with_capacity(findings.len());
    let mut rule_ids: Vec<Option<String>> = Vec::with_capacity(findings.len());
    let mut messages = Vec::with_capacity(findings.len());
    let mut suggested_fixes: Vec<Option<String>> = Vec::with_capacity(findings.len());
    let mut code_snippets: Vec<Option<String>> = Vec::with_capacity(findings.len());
    let mut contexts: Vec<Option<String>> = Vec::with_capacity(findings.len());
    let mut categories: Vec<Option<String>> = Vec::with_capacity(findings.len());

    for f in findings {
        review_ids.push(f.review_id);
        fingerprints.push(f.fingerprint.clone());
        file_paths.push(f.file_path.clone());
        line_starts.push(f.line_start);
        line_ends.push(f.line_end);
        column_starts.push(f.column_start);
        column_ends.push(f.column_end);
        severities.push(f.severity.clone());
        detectors.push(f.detector.clone());
        rule_ids.push(f.rule_id.clone());
        messages.push(f.message.clone());
        suggested_fixes.push(f.suggested_fix.clone());
        code_snippets.push(f.code_snippet.clone());
        contexts.push(f.context.clone());
        categories.push(f.category.clone());
    }

    sqlx::query(
        "INSERT INTO findings (
            review_id, fingerprint, file_path, line_start, line_end,
            column_start, column_end, severity, detector, rule_id,
            message, suggested_fix, code_snippet, context, category
         )
         SELECT * FROM UNNEST(
            $1::bigint[], $2::text[], $3::text[], $4::bigint[], $5::bigint[],
            $6::bigint[], $7::bigint[], $8::text[], $9::text[], $10::text[],
            $11::text[], $12::text[], $13::text[], $14::text[], $15::text[]
         )",
    )
    .bind(&review_ids)
    .bind(&fingerprints)
    .bind(&file_paths)
    .bind(&line_starts)
    .bind(&line_ends)
    .bind(&column_starts)
    .bind(&column_ends)
    .bind(&severities)
    .bind(&detectors)
    .bind(&rule_ids)
    .bind(&messages)
    .bind(&suggested_fixes)
    .bind(&code_snippets)
    .bind(&contexts)
    .bind(&categories)
    .execute(pool.as_pg())
    .await?;

    Ok(())
}

pub async fn get_stats(pool: &DbPool) -> Result<serde_json::Value, sqlx::Error> {
    let total_repos: i64 = db_scalar!(
        pool,
        i64,
        "SELECT COUNT(*) FROM repos WHERE active = ?",
        true
    )?;

    let total_reviews_today: i64 = db_scalar!(
        pool,
        i64,
        "SELECT COUNT(*) FROM reviews
         WHERE created_at >= CURRENT_DATE
           AND created_at < CURRENT_DATE + INTERVAL '1 day'"
    )?;

    let pass_rate: Option<f64> = db_scalar!(
        pool,
        Option<f64>,
        "SELECT AVG(CASE WHEN status = 'passed' THEN 100.0 WHEN status = 'failed' THEN 0.0 ELSE NULL END)
         FROM reviews
         WHERE created_at >= NOW() - INTERVAL '30 days'"
    )?;

    let total_findings: i64 = db_scalar!(
        pool,
        i64,
        "SELECT COUNT(*) FROM findings f
         INNER JOIN reviews r ON r.id = f.review_id
         WHERE r.created_at >= NOW() - INTERVAL '30 days'"
    )?;

    let reviews_last_7_days: i64 = db_scalar!(
        pool,
        i64,
        "SELECT COUNT(*) FROM reviews WHERE created_at >= NOW() - INTERVAL '7 days'"
    )?;

    let dismissals_last_7_days: i64 = db_scalar!(
        pool,
        i64,
        "SELECT COUNT(*) FROM dismissed_findings WHERE dismissed_at >= NOW() - INTERVAL '7 days'"
    )?;

    Ok(serde_json::json!({
        "total_repos": total_repos,
        "total_reviews_today": total_reviews_today,
        "pass_rate": pass_rate,
        "total_findings": total_findings,
        "reviews_last_7_days": reviews_last_7_days,
        "dismissals_last_7_days": dismissals_last_7_days,
    }))
}
