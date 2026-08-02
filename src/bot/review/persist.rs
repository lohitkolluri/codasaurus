use crate::detectors::Findings;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_review_to_db(
    repo_name: &str,
    pr_number: i64,
    pr_title: &str,
    pr_author: &str,
    base_branch: &str,
    head_branch: &str,
    head_sha: &str,
    findings: &Findings,
    has_blocking: bool,
) {
    let pool = match crate::bot::CONFIG_POOL.get() {
        Some(p) => p,
        None => return,
    };
    let repo_id = match crate::db::repos::get_repo_by_full_name(pool, repo_name).await {
        Ok(Some(r)) => r.id,
        _ => return,
    };
    let review = match crate::db::reviews::create_review(
        pool,
        &crate::db::models::ReviewCreate {
            repo_id,
            pr_number,
            pr_title: Some(pr_title.to_string()),
            pr_author: if pr_author.is_empty() {
                None
            } else {
                Some(pr_author.to_string())
            },
            pr_base_branch: if base_branch.is_empty() {
                None
            } else {
                Some(base_branch.to_string())
            },
            pr_head_branch: if head_branch.is_empty() {
                None
            } else {
                Some(head_branch.to_string())
            },
            pr_head_sha: if head_sha.is_empty() {
                None
            } else {
                Some(head_sha.to_string())
            },
        },
    )
    .await
    {
        Ok(r) => r,
        Err(_) => return,
    };
    // Upsert reuses the same review id — drop stale findings first.
    if let Err(e) = crate::db::reviews::delete_findings_for_review(pool, review.id).await {
        eprintln!("Warning: failed to clear prior findings: {e}");
    }
    let batch: Vec<crate::db::models::FindingCreate> = findings
        .findings
        .iter()
        .map(|f| crate::db::models::FindingCreate {
            review_id: review.id,
            fingerprint: Some(format!("{}:{}", review.id, f.fingerprint())),
            file_path: f.file.clone(),
            line_start: if f.line > 0 {
                Some(f.line as i32)
            } else {
                None
            },
            line_end: None,
            column_start: None,
            column_end: None,
            severity: f.severity.to_string(),
            detector: f.detector.clone(),
            rule_id: None,
            message: crate::bot::markdown::redact_secrets(&f.message),
            suggested_fix: f
                .suggestion
                .as_ref()
                .map(|s| crate::bot::markdown::redact_secrets(s)),
            code_snippet: f
                .codemod
                .as_ref()
                .map(|s| crate::bot::markdown::redact_secrets(s)),
            context: None,
            category: None,
            confidence: f.confidence.map(|c| c as i32),
            judge_rationale: f.judge_rationale.clone(),
        })
        .collect();
    if let Err(e) = crate::db::reviews::create_findings_batch(pool, &batch).await {
        eprintln!("Warning: failed to persist findings batch: {e}");
    }
    let tier1 = findings
        .findings
        .iter()
        .filter(|f| matches!(f.detector.as_str(), "secrets" | "vulnerabilities" | "iac"))
        .count();
    crate::metrics::record_tier1_findings(tier1);
    let status = if has_blocking { "failed" } else { "passed" };
    if let Err(e) = crate::db::reviews::update_review(
        pool,
        review.id,
        &crate::db::models::ReviewUpdate {
            status: Some(status.to_string()),
            summary_json: None,
            completed_at: Some(chrono::Utc::now()),
        },
    )
    .await
    {
        eprintln!("Warning: failed to update review status: {e}");
    }
    crate::db::audit::log_event(
        pool,
        &format!("review.{status}"),
        Some(pr_author),
        Some("review"),
        Some(review.id),
    )
    .await;
}
