use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use crate::state::ReviewState;
use anyhow::Result;
use std::sync::LazyLock;
use std::time::Duration;

const PER_PAGE: usize = crate::util::github::PR_FILES_PER_PAGE;
/// GitHub exposes at most 3,000 PR files (30 pages of 100).
const MAX_PR_FILE_PAGES: usize = crate::util::github::MAX_PR_FILE_PAGES;

/// Build a production-configured GitHub API client with timeouts and pooling.
pub(crate) static GITHUB_CLIENT: LazyLock<Option<reqwest::Client>> = LazyLock::new(|| {
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
pub(crate) fn github_api_headers(auth_header: &str) -> Result<reqwest::header::HeaderMap> {
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
pub(crate) async fn fetch_pr_files(
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

/// Post or update an issue comment using a named slot for idempotency
/// (`walkthrough`, `llm_summary`, `describe`, …) so slots never overwrite each other.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn post_or_update_comment(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    pr_number: i64,
    body: &str,
    state: &Option<ReviewState>,
    kind: &str,
) -> Result<i64> {
    let url = format!("https://api.github.com/repos/{repo_name}/issues/{pr_number}/comments");

    let headers = github_api_headers(auth_header)?;

    if let Some(ref s) = state {
        match s.get_comment_id_async(repo_name, pr_number, kind).await {
            Ok(Some(comment_id)) => {
                let update_url = format!(
                    "https://api.github.com/repos/{repo_name}/issues/comments/{comment_id}"
                );
                match retry_async(
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
                {
                    Ok(resp) if resp.status().is_success() => {
                        tracing::info!(%kind, comment_id, "updated existing PR comment in place");
                        return Ok(comment_id);
                    }
                    Ok(resp) => {
                        tracing::warn!(
                            %kind,
                            comment_id,
                            status = %resp.status(),
                            "failed to update comment — posting a new one"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            %kind,
                            comment_id,
                            error = %e,
                            "failed to update comment — posting a new one"
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                // Do not treat DB failure as "no slot" silently — log so operators notice
                // duplicate comments when persistence is down.
                tracing::warn!(
                    %kind,
                    error = %e,
                    "failed to load comment slot; posting a new comment"
                );
            }
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
            if let Err(e) = s
                .set_comment_id_async(repo_name, pr_number, kind, comment_id)
                .await
            {
                tracing::warn!(%kind, comment_id, error = %e, "failed to store comment ID");
            }
        }
    }

    Ok(comment_id)
}
