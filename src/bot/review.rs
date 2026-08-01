use crate::bot::WebhookPayload;
use crate::detectors::{self, Finding, Findings};
use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use crate::state::ReviewState;
use anyhow::Result;
use std::fmt::Write;
use std::sync::LazyLock;
use std::time::Duration;

/// GitHub API max results per page for PR files.
const PER_PAGE: usize = 100;
/// GitHub exposes at most 3,000 PR files (30 pages of 100).
const MAX_PR_FILE_PAGES: usize = 30;
/// Bound reviewer discovery to avoid exhausting an installation's API quota on a large PR.
const MAX_REVIEWER_FILES: usize = 8;

/// Build a production-configured GitHub API client with timeouts and pooling.
static GITHUB_CLIENT: LazyLock<Option<reqwest::Client>> = LazyLock::new(|| {
    match reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(180))
        .tcp_nodelay(true)
        .build()
    {
        Ok(client) => Some(client),
        Err(e) => {
            eprintln!("Warning: failed to build GitHub API client: {e}");
            None
        }
    }
});

/// Fetch the complete PR object for a comment-triggered review.
pub async fn fetch_pull_request(
    token: &str,
    repo_name: &str,
    pr_number: i64,
) -> Result<serde_json::Value> {
    let client = GITHUB_CLIENT
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("GitHub API client not available (failed to initialize)"))?;
    let auth_header = format!("Bearer {token}");
    let headers = github_api_headers(&auth_header)?;
    let url = format!("https://api.github.com/repos/{repo_name}/pulls/{pr_number}");
    retry_async(
        &RetryConfig::api_default(),
        "fetch_pull_request",
        &is_reqwest_error_retryable,
        || async {
            client
                .get(&url)
                .headers(headers.clone())
                .send()
                .await?
                .error_for_status()?
                .json()
                .await
                .map_err(Into::into)
        },
    )
    .await
}

/// Same auth/User-Agent headers reused across all GitHub API calls.
fn github_api_headers(auth_header: &str) -> Result<reqwest::header::HeaderMap> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(auth_header)
            .map_err(|e| anyhow::anyhow!("Invalid GitHub auth token: {e}"))?,
    );
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(concat!(
            "codasaurus/",
            env!("CARGO_PKG_VERSION")
        )),
    );
    Ok(headers)
}

/// Fetch all changed files for a PR. GitHub caps this endpoint at 3,000 files.
async fn fetch_pr_files(
    client: &reqwest::Client,
    repo_name: &str,
    pr_number: i64,
    auth_header: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut all_files = Vec::new();
    for page_number in 1..=MAX_PR_FILE_PAGES {
        let url = format!(
            "https://api.github.com/repos/{repo_name}/pulls/{pr_number}/files?per_page={PER_PAGE}&page={page_number}"
        );
        let page: Vec<serde_json::Value> = retry_async(
            &RetryConfig::api_default(),
            "fetch_pr_files_page",
            &is_reqwest_error_retryable,
            || async {
                let headers = github_api_headers(auth_header)?;
                client
                    .get(&url)
                    .headers(headers)
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await
                    .map_err(Into::into)
            },
        )
        .await?;
        let is_last_page = page.len() < PER_PAGE;
        all_files.extend(page);
        if is_last_page || page_number == MAX_PR_FILE_PAGES {
            return Ok(all_files);
        }
    }

    unreachable!("the bounded page loop always returns")
}

/// Post or update an issue comment using state store for idempotency.
async fn post_or_update_comment(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    pr_number: i64,
    body: &str,
    state: &Option<ReviewState>,
) -> Result<i64> {
    let url = format!("https://api.github.com/repos/{repo_name}/issues/{pr_number}/comments");

    let headers = github_api_headers(auth_header)?;

    if let Some(ref s) = state {
        if let Ok(Some(comment_id)) = s.get_comment_id_async(repo_name, pr_number).await {
            let update_url =
                format!("https://api.github.com/repos/{repo_name}/issues/comments/{comment_id}");
            let update_ok = retry_async(
                &RetryConfig::api_default(),
                "update_comment",
                &is_reqwest_error_retryable,
                || async {
                    client
                        .patch(&update_url)
                        .headers(headers.clone())
                        .json(&serde_json::json!({"body": body}))
                        .send()
                        .await
                        .map_err(Into::into)
                },
            )
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
            if update_ok {
                return Ok(comment_id);
            }
            eprintln!("Warning: failed to update comment {comment_id} , creating new",);
        }
    }

    let resp: serde_json::Value = retry_async(
        &RetryConfig::api_default(),
        "create_comment",
        &is_reqwest_error_retryable,
        || async {
            client
                .post(&url)
                .headers(headers.clone())
                .json(&serde_json::json!({"body": body}))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await
                .map_err(Into::into)
        },
    )
    .await?;

    let comment_id = resp["id"].as_i64().unwrap_or(0);
    if comment_id > 0 {
        if let Some(ref s) = state {
            if let Err(e) = s.set_comment_id_async(repo_name, pr_number, comment_id).await {
                eprintln!("Warning: failed to store comment ID: {e}");
            }
        }
    }

    Ok(comment_id)
}

/// Suggest reviewers based on git history for changed files.
/// Fires parallel API calls (capped via semaphore) to find recent authors.
async fn suggest_reviewers(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    files: &[serde_json::Value],
    pr_author: &str,
    max_files: usize,
) -> Vec<String> {
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    let max_files = max_files.clamp(0, MAX_REVIEWER_FILES).max(0);
    if max_files == 0 || files.is_empty() {
        return Vec::new();
    }

    let author_counts = Arc::new(std::sync::Mutex::new(HashMap::<String, usize>::new()));
    let semaphore = Arc::new(Semaphore::new(5)); // keep GitHub fan-out modest

    let mut handles = Vec::with_capacity(files.len().min(max_files));
    for file in files.iter().take(max_files) {
        let filename = match file["filename"].as_str() {
            Some(f) if !f.is_empty() => f.to_string(),
            _ => continue,
        };
        let cl = client.clone();
        let auth = auth_header.to_string();
        let repo = repo_name.to_string();
        let author = pr_author.to_string();
        let counts = Arc::clone(&author_counts);
        let sem = Arc::clone(&semaphore);

        handles.push(tokio::spawn(async move {
            let _permit = match sem.acquire().await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Warning: semaphore closed: {e}");
                    return;
                }
            };

            let encoded_path = urlencoding_encode(&filename);
            let commits_url = format!(
                "https://api.github.com/repos/{repo}/commits?path={encoded_path}&per_page=3"
            );
            let commits: Vec<serde_json::Value> = match crate::retry::retry_async(
                &crate::retry::RetryConfig::quick(),
                "suggest_reviewer_commits",
                &crate::retry::is_reqwest_error_retryable,
                || async {
                    cl.get(&commits_url)
                        .header("Authorization", &auth)
                        .header("Accept", "application/vnd.github+json")
                        .header(
                            "User-Agent",
                            concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
                        )
                        .send()
                        .await
                        .map_err(Into::into)
                },
            )
            .await
            {
                Ok(resp) => match resp.error_for_status() {
                    Ok(r) => r.json::<Vec<serde_json::Value>>().await.unwrap_or_default(),
                    Err(_) => vec![],
                },
                Err(_) => vec![],
            };

            // _permit dropped here → semaphore permit returned automatically

            if !commits.is_empty() {
                let mut local = counts.lock().unwrap();
                for commit in &commits {
                    if let Some(login) = commit["author"]["login"].as_str() {
                        if login != author {
                            *local.entry(login.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }));
    }

    // Wait for all fetches to complete
    for h in handles {
        let _ = h.await;
    }

    let counts = author_counts.lock().unwrap();
    let mut reviewers: Vec<(String, usize)> = counts.clone().into_iter().collect();
    reviewers.sort_by_key(|k| std::cmp::Reverse(k.1));
    reviewers.truncate(5);
    reviewers.into_iter().map(|(name, _)| name).collect()
}

/// True if finding severity meets the configured minimum (blocking > warning > info).
fn severity_at_least(sev: &str, min: &str) -> bool {
    fn rank(s: &str) -> u8 {
        match s {
            "blocking" => 3,
            "warning" => 2,
            _ => 1,
        }
    }
    rank(sev) >= rank(min)
}

pub async fn review_pr(token: &str, repo_name: &str, payload: &WebhookPayload) -> Result<()> {
    let pr = match &payload.pull_request {
        Some(p) => p,
        None => return Ok(()),
    };

    // Draft PRs: skip full review (edge case) unless explicitly forced via comment path
    let is_draft = pr["draft"].as_bool().unwrap_or(false);
    if is_draft {
        tracing::info!(repo = repo_name, "skipping draft PR");
        return Ok(());
    }

    let pr_number = pr["number"].as_i64().unwrap_or(0);
    let pr_title = pr["title"].as_str().unwrap_or("").to_string();
    let pr_body = pr["body"].as_str().unwrap_or("").to_string();
    let head_sha = pr["head"]["sha"].as_str().unwrap_or("");

    let pool = crate::bot::CONFIG_POOL.get();

    // Honor dashboard active toggle
    if let Some(pool) = pool {
        if let Ok(Some(repo)) = crate::db::repos::get_repo_by_full_name(pool, repo_name).await {
            if !repo.active {
                tracing::info!(repo = repo_name, "repo inactive — skipping review");
                return Ok(());
            }
        }
    }

    let state = pool
        .map(ReviewState::from_pool)
        .or_else(|| ReviewState::open().ok());

    // Claim SHA at start to close TOCTOU race between concurrent workers
    if !head_sha.is_empty() {
        if let Some(ref s) = state {
            match s.try_claim_sha(repo_name, pr_number, head_sha).await {
                Ok(false) => {
                    tracing::info!(repo = repo_name, pr = pr_number, sha = head_sha, "skipping already-claimed SHA");
                    return Ok(());
                }
                Ok(true) => {}
                Err(e) => tracing::warn!(error = %e, "SHA claim failed; continuing"),
            }
        }
    }

    let mut config = crate::config::Config::load_for_bot(pool).await;
    let mut repo_llm_enabled = true;
    if let Some(pool) = pool {
        if let Ok(Some(repo)) = crate::db::repos::get_repo_by_full_name(pool, repo_name).await {
            if let Some(ref cfg_json) = repo.config_json {
                if let Some(llm) = config.overlay_repo_config_json(cfg_json) {
                    repo_llm_enabled = llm;
                }
            }
        }
    }
    let policy = crate::config::Config::bot_policy(pool).await;
    let runtime = crate::bot_runtime::BotRuntimeConfig::default();

    // Keep local FS guidelines off; remote guidelines applied after Contents API fetch.
    let want_guidelines = config.checks.guidelines;
    config.checks.guidelines = false;

    let client = GITHUB_CLIENT
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("GitHub API client not available (failed to initialize)"))?;
    let auth_header = format!("Bearer {token}");
    let headers = github_api_headers(&auth_header)?;

    let base_sha = pr["base"]["sha"].as_str().unwrap_or("");
    let head_ref = pr["head"]["ref"].as_str().unwrap_or("");

    let files = fetch_pr_files(client, repo_name, pr_number, &auth_header)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to fetch PR files");
            e
        })?;
    if files.is_empty() {
        tracing::info!(repo = repo_name, pr = pr_number, "no files in PR");
        return Ok(());
    }

    let changed_paths: Vec<String> = files
        .iter()
        .filter_map(|f| f["filename"].as_str().map(String::from))
        .collect();

    let mut parsed_files_collected: Vec<crate::parser::ParsedFile> = Vec::new();
    let mut already_have = std::collections::HashSet::new();
    for file in &files {
        let filename = file["filename"].as_str().unwrap_or("unknown");
        already_have.insert(filename.to_string());
        let patch = file["patch"].as_str().unwrap_or("");
        if !patch.is_empty() && patch.len() < 100_000 {
            let parsed = match crate::parser::parse_unified_diff(filename, patch) {
                Ok(p) => Some(p),
                Err(e) => {
                    eprintln!("Warning: failed to parse file {filename}: {e}");
                    None
                }
            };
            if let Some(p) = parsed {
                parsed_files_collected.push(p);
            }
        }
    }

    // Fetch commits early — used by slop + remote guidelines.
    let pr_author = pr["user"]["login"].as_str().unwrap_or("");
    let commits_url = format!("https://api.github.com/repos/{repo_name}/pulls/{pr_number}/commits");
    let commit_messages: Vec<String> = match retry_async(
        &RetryConfig::api_default(),
        "fetch_pr_commits",
        &is_reqwest_error_retryable,
        || async {
            client
                .get(&commits_url)
                .headers(headers.clone())
                .send()
                .await
                .map_err(Into::into)
        },
    )
    .await
    {
        Ok(r) => match r.json::<Vec<serde_json::Value>>().await {
            Ok(commits) => commits
                .iter()
                .filter_map(|c| c["commit"]["message"].as_str().map(String::from))
                .collect(),
            Err(_) => vec![],
        },
        Err(_) => vec![],
    };

    // Repo awareness: manifests, CONTRIBUTING/AGENTS, CODEOWNERS, linked issues.
    let (remote_ctx, bootstrapped) = crate::bot::repo_context::gather_remote_context(
        client,
        &headers,
        repo_name,
        base_sha,
        head_ref,
        &pr_title,
        &pr_body,
        &changed_paths,
        &already_have,
    )
    .await
    .unwrap_or_default();

    // Prefer full base-branch manifests over incomplete patch slices.
    for m in bootstrapped {
        parsed_files_collected.retain(|p| p.path != m.path);
        parsed_files_collected.push(m);
    }
    if remote_ctx.manifests_added > 0 {
        tracing::info!(
            n = remote_ctx.manifests_added,
            "bootstrapped dependency manifests from base branch"
        );
    }

    // Warm registry/OSV caches concurrently before sync detectors run.
    let prefetch_pairs = collect_registry_pairs(&parsed_files_collected);
    if !prefetch_pairs.is_empty() {
        crate::registry::prefetch_packages(&prefetch_pairs).await;
    }

    // Detectors stay sync but registry hits are now mostly cache; still isolate on a worker.
    let mut findings = if parsed_files_collected.is_empty() {
        Findings::new()
    } else {
        let cfg = config.clone();
        let parsed = parsed_files_collected.clone();
        tokio::task::spawn_blocking(move || detectors::run_all(&parsed, &cfg))
            .await
            .map_err(|e| anyhow::anyhow!("detector task join error: {e}"))?
    };

    if want_guidelines && !remote_ctx.guidelines.is_empty() {
        let g = detectors::guidelines::detect_remote(
            &remote_ctx.guidelines,
            head_ref,
            &commit_messages,
            &changed_paths,
        );
        findings.findings.extend(g);
    }

    // Apply default_severity floor from dashboard settings
    findings
        .findings
        .retain(|f| severity_at_least(f.severity, &policy.min_severity));

    let slop_findings = crate::detectors::slop::detect_slop(
        &parsed_files_collected,
        &pr_title,
        &pr_body,
        &commit_messages,
    );
    findings.findings.extend(slop_findings);

    let mut reviewers = suggest_reviewers(
        client,
        &auth_header,
        repo_name,
        &files,
        pr_author,
        runtime.max_reviewer_files.min(MAX_REVIEWER_FILES),
    )
    .await;
    // CODEOWNERS first, then history-based (deduped).
    for owner in remote_ctx.codeowner_reviewers.iter().rev() {
        if owner != pr_author && !reviewers.iter().any(|r| r == owner) {
            reviewers.insert(0, owner.clone());
        }
    }
    reviewers.truncate(8);

    let review_ctx = crate::bot::repo_context::to_review_context(
        &remote_ctx,
        repo_name,
        head_ref,
        &pr_title,
        &pr_body,
    );

    let mut review_comments: Vec<serde_json::Value> = Vec::new();
    let mut has_blocking = false;
    let mut seen_detectors: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    // Merge vulnerability findings on the same (file, line) to avoid spam.
    // One package with 9 CVEs → one inline comment, not nine.
    let display_findings = merge_vulnerability_findings(&findings.findings);

    // Sort by severity: blocking first, then warning, then info
    let mut prioritized: Vec<&Finding> = display_findings.iter().collect();
    prioritized.sort_by_key(|f| match f.severity {
        "blocking" => 0,
        "warning" => 1,
        _ => 2,
    });

    for f in &prioritized {
        if f.severity == "blocking" {
            has_blocking = true;
        }
        if f.line == 0 {
            continue;
        }
        if review_comments.len() >= runtime.max_inline_comments {
            break;
        }

        // Dedup: only 1 inline comment per (file, detector) pair.
        let key = (f.file.clone(), f.detector.clone());
        if !seen_detectors.insert(key) {
            continue;
        }

        let comment_body = crate::bot::markdown::inline_finding_comment(f);
        let comment = serde_json::json!({
            "path": f.file,
            "line": f.line,
            "side": "RIGHT",
            "body": comment_body,
        });
        review_comments.push(comment);
    }

    // Only approve when there are genuinely no findings.
    if findings.is_empty() {
        let body = crate::bot::markdown::clean_approve_body();
        let review = serde_json::json!({"body": body, "event": "APPROVE"});
        let approve_url =
            format!("https://api.github.com/repos/{repo_name}/pulls/{pr_number}/reviews");
        let _: serde_json::Value = retry_async(
            &RetryConfig::api_default(),
            "approve_review",
            &is_reqwest_error_retryable,
            || async {
                client
                    .post(&approve_url)
                    .header("Authorization", &auth_header)
                    .header("Accept", "application/vnd.github+json")
                    .header(
                        "User-Agent",
                        concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
                    )
                    .json(&review)
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await
                    .map_err(Into::into)
            },
        )
        .await?;
        if !head_sha.is_empty() {
            if let Some(ref s) = state {
                if let Err(e) = s.set_reviewed_sha_async(repo_name, pr_number, head_sha).await {
                    eprintln!("Warning: failed to store reviewed SHA: {e}");
                };
            }
        }
        // Persist clean review to local DB
        save_review_to_db(
            repo_name,
            pr_number,
            &pr_title,
            pr["user"]["login"].as_str().unwrap_or(""),
            pr["base"]["ref"].as_str().unwrap_or(""),
            pr["head"]["ref"].as_str().unwrap_or(""),
            head_sha,
            &findings,
            false,
        )
        .await;
        return Ok(());
    }

    let body = crate::bot::markdown::walkthrough_body(
        &findings,
        has_blocking,
        &pr_title,
        &files,
        &reviewers,
        &config,
        &runtime,
        false,
    );

    // Try to create a review with inline comments; fall back to single comment
    let review_body = serde_json::json!({
        "body": body,
        "event": if has_blocking { "REQUEST_CHANGES" } else { "COMMENT" },
        "comments": review_comments,
    });

    let review_url = format!("https://api.github.com/repos/{repo_name}/pulls/{pr_number}/reviews");
    let resp = retry_async(
        &RetryConfig::api_default(),
        "post_pr_review",
        &is_reqwest_error_retryable,
        || async {
            client
                .post(&review_url)
                .header("Authorization", &auth_header)
                .header("Accept", "application/vnd.github+json")
                .header(
                    "User-Agent",
                    concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
                )
                .json(&review_body)
                .send()
                .await
                .map_err(Into::into)
        },
    )
    .await?;

    // If inline review failed (e.g. line numbers don't match), fall back to a single issue comment.
    // Uses the state store to update the previous comment rather than posting a new one.
    if !resp.status().is_success() {
        post_or_update_comment(client, &auth_header, repo_name, pr_number, &body, &state).await?;
    }

    // Record the reviewed commit SHA for incremental review
    if !head_sha.is_empty() {
        if let Some(ref s) = state {
            if let Err(e) = s.set_reviewed_sha_async(repo_name, pr_number, head_sha).await {
                eprintln!("Warning: failed to store reviewed SHA: {e}");
            };
        }
    }

    // Persist review + findings to local DB for dashboard and audit log
    save_review_to_db(
        repo_name,
        pr_number,
        &pr_title,
        pr["user"]["login"].as_str().unwrap_or(""),
        pr["base"]["ref"].as_str().unwrap_or(""),
        pr["head"]["ref"].as_str().unwrap_or(""),
        head_sha,
        &findings,
        has_blocking,
    )
    .await;

    // Generate and post LLM summary if enabled for this repo and an API key is available
    if repo_llm_enabled {
        if let Some(llm_cfg) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            if let Err(e) = generate_and_post_summary(
                client,
                &auth_header,
                repo_name,
                pr_number,
                &findings,
                &llm_cfg,
                &pr_title,
                &pr_body,
                &state,
                &review_ctx,
            )
            .await
            {
                tracing::warn!(error = %e, "failed to generate LLM summary");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
fn build_comment_body(finding: &Finding) -> String {
    crate::bot::markdown::inline_finding_comment(finding)
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Unique (registry, package) pairs for concurrent cache warming.
fn collect_registry_pairs(files: &[crate::parser::ParsedFile]) -> Vec<(String, String)> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for file in files {
        let registry = match file.language.as_str() {
            "javascript" | "typescript" | "tsx" | "jsx" => "npm",
            "python" => "pypi",
            "rust" => "crates.io",
            _ => continue,
        };
        for import in &file.imports {
            let Some(package) = crate::detectors::extract_package_name(&import.name) else {
                continue;
            };
            if package.starts_with('.') || package.starts_with('/') {
                continue;
            }
            if crate::detectors::hallucinated_imports::is_builtin(&package, registry) {
                continue;
            }
            let key = (registry.to_string(), package);
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
async fn generate_and_post_summary(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    pr_number: i64,
    findings: &Findings,
    llm_cfg: &crate::llm::LlmConfig,
    pr_title: &str,
    pr_body: &str,
    state: &Option<ReviewState>,
    review_ctx: &crate::llm::ReviewContext,
) -> Result<()> {
    let mut findings_text = String::new();
    if let Some(ref ctx) = review_ctx.repo_context {
        let _ = writeln!(findings_text, "Repo context:\n{ctx}\n");
    }
    if !review_ctx.linked_issues.is_empty() {
        let _ = writeln!(findings_text, "Linked issues:");
        for iss in &review_ctx.linked_issues {
            let _ = writeln!(findings_text, "- #{} {}", iss.number, iss.title);
        }
        findings_text.push('\n');
    }
    // Prefer blocking findings first; cap volume for token cost.
    let mut ordered: Vec<&Finding> = findings.findings.iter().collect();
    ordered.sort_by_key(|f| match f.severity {
        "blocking" => 0,
        "warning" => 1,
        _ => 2,
    });
    for f in ordered.iter().take(40) {
        let _ = writeln!(
            findings_text,
            "- {}: {} (line {})",
            f.severity, f.message, f.line
        );
    }

    let summary = crate::llm::summarize_pr(pr_title, pr_body, &findings_text, llm_cfg).await?;

    let summary_body = format!(
        "### AI review summary\n\n{summary}\n\n---\n_Generated by Codasaurus_"
    );

    post_or_update_comment(
        client,
        auth_header,
        repo_name,
        pr_number,
        &summary_body,
        state,
    )
    .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn save_review_to_db(
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
    let batch: Vec<crate::db::models::FindingCreate> = findings
        .findings
        .iter()
        .map(|f| crate::db::models::FindingCreate {
            review_id: review.id,
            fingerprint: Some(format!("{}:{}", review.id, f.fingerprint())),
            file_path: f.file.clone(),
            line_start: if f.line > 0 {
                Some(f.line as i64)
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
            suggested_fix: f.suggestion.clone(),
            code_snippet: f.codemod.clone(),
            context: None,
            category: None,
        })
        .collect();
    if let Err(e) = crate::db::reviews::create_findings_batch(pool, &batch).await {
        eprintln!("Warning: failed to persist findings batch: {e}");
    }
    let status = if has_blocking { "failed" } else { "passed" };
    if let Err(e) = crate::db::reviews::update_review(
        pool,
        review.id,
        &crate::db::models::ReviewUpdate {
            status: Some(status.to_string()),
            summary_json: None,
            completed_at: Some(chrono::Utc::now().to_rfc3339()),
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

fn merge_vulnerability_findings(
    findings: &[crate::detectors::Finding],
) -> Vec<crate::detectors::Finding> {
    use crate::detectors::Finding;
    use std::collections::BTreeMap;

    let mut groups: BTreeMap<(String, usize), Vec<&Finding>> = BTreeMap::new();
    let mut non_vuln: Vec<&Finding> = Vec::new();

    for f in findings {
        if f.detector == "vulnerabilities" && f.line > 0 {
            groups.entry((f.file.clone(), f.line)).or_default().push(f);
        } else {
            non_vuln.push(f);
        }
    }

    let mut result: Vec<Finding> = non_vuln.into_iter().cloned().collect();

    for ((file, line), group) in groups {
        if group.len() <= 1 {
            result.extend(group.into_iter().cloned());
            continue;
        }
        let max_sev: &str = group
            .iter()
            .map(|f| f.severity)
            .max_by_key(|s| match *s {
                "blocking" => 3,
                "warning" => 2,
                _ => 1,
            })
            .unwrap_or("info");
        let cve_list: Vec<&str> = group
            .iter()
            .filter_map(|f| f.message.split(':').next())
            .collect();
        let count = group.len();
        result.push(Finding {
            file,
            line,
            column: 0,
            severity: match max_sev {
                "blocking" => "blocking",
                "warning" => "warning",
                _ => "info",
            },
            detector: "vulnerabilities".into(),
            message: format!(
                "{} known CVE{}: {}",
                count,
                if count == 1 { "" } else { "s" },
                cve_list.join(", ")
            ),
            suggestion: group.first().and_then(|f| f.suggestion.clone()),
            codemod: None,
            evidence: None,
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::Finding;

    fn f(
        detector: &str,
        severity: &'static str,
        file: &str,
        line: usize,
        msg: &str,
        sug: Option<&str>,
    ) -> Finding {
        Finding {
            detector: detector.into(),
            severity,
            file: file.into(),
            line,
            column: 0,
            message: msg.into(),
            suggestion: sug.map(|s| s.into()),
            evidence: None,
            codemod: None,
        }
    }

    #[test]
    fn comment_body_hallucinated_import() {
        let body = build_comment_body(&f(
            "hallucinated-imports",
            "blocking",
            "src/a.ts",
            5,
            "Package `fakelib` not found on npm.",
            Some("Check npmjs.com"),
        ));
        assert!(body.contains("Package does not exist"));
        assert!(body.contains("fakelib"));
        assert!(body.contains("fingerprint:"));
        assert!(body.contains("`blocking`"));
    }

    #[test]
    fn comment_body_secret() {
        let body = build_comment_body(&f(
            "secrets",
            "blocking",
            "src/x.ts",
            10,
            "API Key detected",
            Some("Use env vars"),
        ));
        assert!(body.contains("Credential in source"));
        assert!(body.contains("secret") || body.contains("Rotate"));
    }

    #[test]
    fn comment_body_vulnerability() {
        let body = build_comment_body(&f(
            "vulnerabilities",
            "warning",
            "pkg.json",
            7,
            "GHSA-123: desc",
            Some("Update `lodash`"),
        ));
        assert!(body.contains("Known vulnerability"));
        assert!(body.contains("lodash"));
    }

    #[test]
    fn comment_body_todo() {
        let body = build_comment_body(&f(
            "todo-leaks",
            "warning",
            "src/a.ts",
            15,
            "// TODO: fix",
            Some("Complete it"),
        ));
        assert!(body.contains("Incomplete code"));
    }

    #[test]
    fn merge_vulns_collapses_same_line() {
        let findings = vec![
            f(
                "vulnerabilities",
                "warning",
                "x.json",
                1,
                "CVE-1: d1",
                Some("Up `lodash`"),
            ),
            f(
                "vulnerabilities",
                "warning",
                "x.json",
                1,
                "CVE-2: d2",
                Some("Up `lodash`"),
            ),
            f(
                "hallucinated-imports",
                "blocking",
                "a.ts",
                5,
                "not found",
                Some("Check npm"),
            ),
        ];
        let merged = merge_vulnerability_findings(&findings);
        assert_eq!(merged.len(), 2); // 2 vulns merged + 1 non-vuln
    }

    #[test]
    fn merge_vulns_keeps_single() {
        let findings = vec![
            f(
                "vulnerabilities",
                "warning",
                "x.json",
                1,
                "CVE-1: d1",
                Some("Up `lodash`"),
            ),
            f(
                "vulnerabilities",
                "blocking",
                "y.json",
                2,
                "CVE-2: d2",
                Some("Up `zod`"),
            ),
        ];
        let merged = merge_vulnerability_findings(&findings);
        assert_eq!(merged.len(), 2); // different files, NOT merged
    }

    #[test]
    fn extract_package_from_backtick_msg() {
        fn pkg(msg: &str) -> String {
            msg.split('`').nth(1).unwrap_or("unknown").to_string()
        }
        assert_eq!(pkg("Package `lodash` not found"), "lodash");
        assert_eq!(pkg("no backtick"), "unknown");
    }
}
