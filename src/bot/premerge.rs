//! Natural-language pre-merge checks (Phase 5): LLM-judged, fail-closed offline.

use crate::config::PreMergeCheck;
use crate::db::DbPool;

pub struct PreMergeCheckResult {
    pub name: String,
    pub mode: String,
    pub status: String,
    pub reasoning: String,
}

/// Run configured checks against the diff. Offline or missing LLM config →
/// every check is `inconclusive` (never blocks). Scope-globbed checks only run
/// when a changed path matches.
pub async fn run_pre_merge_checks(
    pool: Option<&DbPool>,
    repo: &str,
    pr_number: i64,
    diff: &str,
    changed_paths: &[String],
    checks: &[PreMergeCheck],
    offline: bool,
) -> Vec<PreMergeCheckResult> {
    let mut results = Vec::new();
    for check in checks {
        if check.mode == "off" {
            continue;
        }
        if let Some(scope) = check.scope.as_deref() {
            if !changed_paths.iter().any(|p| glob_match(scope, p)) {
                continue;
            }
        }

        let (status, reasoning) = if offline {
            (
                "inconclusive".to_string(),
                "LLM disabled (offline mode); inconclusive checks never block".to_string(),
            )
        } else if let Some(llm_cfg) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            match crate::llm::premerge_check(&check.name, &check.instructions, diff, &llm_cfg).await
            {
                Ok((s, r)) => (s, r),
                Err(e) => ("inconclusive".to_string(), format!("LLM error: {e}")),
            }
        } else {
            (
                "inconclusive".to_string(),
                "No LLM configured; pre-merge checks need a BYOK model".to_string(),
            )
        };

        persist(pool, repo, pr_number, check, &status, &reasoning).await;
        results.push(PreMergeCheckResult {
            name: check.name.clone(),
            mode: check.mode.clone(),
            status,
            reasoning,
        });
    }
    results
}

/// Best-effort record of the run (dashboard + audit trail).
async fn persist(
    pool: Option<&DbPool>,
    repo: &str,
    pr_number: i64,
    check: &PreMergeCheck,
    status: &str,
    reasoning: &str,
) {
    let Some(pool) = pool else {
        return;
    };
    let _ = sqlx::query(
        "INSERT INTO pre_merge_check_runs (repo_full_name, pr_number, check_name, mode, status, reasoning, evaluated_at)
         VALUES ($1, $2, $3, $4, $5, $6, NOW())
         ON CONFLICT (repo_full_name, pr_number, check_name)
         DO UPDATE SET status = EXCLUDED.status, reasoning = EXCLUDED.reasoning, evaluated_at = NOW()",
    )
    .bind(repo)
    .bind(pr_number)
    .bind(&check.name)
    .bind(&check.mode)
    .bind(status)
    .bind(reasoning)
    .execute(pool.as_pg())
    .await;
}

pub fn premerge_markdown(results: &[PreMergeCheckResult]) -> String {
    if results.is_empty() {
        return String::new();
    }
    let mut md = String::from("### Pre-merge checks\n\n| Check | Mode | Result |\n|---|---|---|\n");
    for r in results {
        let mark = match r.status.as_str() {
            "passed" => "✅ passed",
            "failed" => "❌ failed",
            _ => "➖ inconclusive",
        };
        md.push_str(&format!("| {} | {} | {} |\n", r.name, r.mode, mark));
        if !r.reasoning.is_empty() {
            md.push_str(&format!(
                "| | | {}\n",
                r.reasoning.chars().take(120).collect::<String>()
            ));
        }
    }
    md.push('\n');
    md
}

/// Minimal glob: `*` matches within a path segment, `**` matches any depth.
fn glob_match(pattern: &str, path: &str) -> bool {
    let pat = pattern.trim_start_matches('/').trim_end_matches('/');
    let p = path.trim_start_matches('/');
    if pat.contains("**") {
        let parts: Vec<&str> = pat.split("**").collect();
        let head = parts.first().unwrap_or(&"").trim_end_matches('/');
        let tail = parts.last().unwrap_or(&"").trim_start_matches('/');
        (head.is_empty() || p.starts_with(head)) && (tail.is_empty() || p.ends_with(tail))
    } else if pat.contains('*') {
        let re_pat = format!("^{}$", regex::escape(pat).replace("\\*", "[^/]*"));
        regex::Regex::new(&re_pat).is_ok_and(|re| re.is_match(p))
    } else {
        p == pat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_matches_paths() {
        assert!(glob_match("tests/**", "tests/auth.rs"));
        assert!(glob_match("tests/**", "tests/deep/unit.rs"));
        assert!(!glob_match("tests/**", "src/auth.rs"));
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "src/main.rs"));
        assert!(glob_match("src/main.rs", "src/main.rs"));
    }

    #[test]
    fn scope_filters_checks() {
        let checks = vec![PreMergeCheck {
            name: "no secrets".into(),
            mode: "error".into(),
            scope: Some("tests/**".into()),
            instructions: "fail on secrets".into(),
        }];
        // No matching path → no results at all.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let results = rt.block_on(run_pre_merge_checks(
            None,
            "acme/repo",
            1,
            "",
            &["src/app.rs".to_string()],
            &checks,
            true,
        ));
        assert!(results.is_empty());
    }

    #[test]
    fn offline_is_inconclusive() {
        let checks = vec![PreMergeCheck {
            name: "no secrets".into(),
            mode: "error".into(),
            scope: None,
            instructions: "fail on secrets".into(),
        }];
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let results = rt.block_on(run_pre_merge_checks(
            None,
            "acme/repo",
            1,
            "diff",
            &["tests/app.rs".to_string()],
            &checks,
            true,
        ));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "inconclusive");
    }
}
