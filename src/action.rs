use crate::retry::{is_reqwest_error_retryable, retry_blocking, RetryConfig};
use anyhow::{Context, Result};

const PR_FILES_PER_PAGE: usize = crate::util::github::PR_FILES_PER_PAGE;
const MAX_PR_FILE_PAGES: usize = crate::util::github::MAX_PR_FILE_PAGES;

/// Run codasaurus as a GitHub Action, posting findings as a Check Run with annotations.
///
/// Reads the GitHub event payload from `GITHUB_EVENT_PATH`, fetches the PR diff,
/// runs detectors on the changed files, and creates/updates a check run with
/// per-finding annotations (capped at 50 per the GitHub API limit).
pub fn run_check_run(event_path: Option<String>) -> Result<()> {
    let event_path = event_path
        .or_else(|| std::env::var("GITHUB_EVENT_PATH").ok())
        .context("No event path provided and GITHUB_EVENT_PATH not set")?;

    let token =
        std::env::var("GITHUB_TOKEN").context("GITHUB_TOKEN environment variable not set")?;

    let event_json =
        std::fs::read_to_string(&event_path).context("Failed to read GITHUB_EVENT_PATH file")?;

    let event: serde_json::Value =
        serde_json::from_str(&event_json).context("Failed to parse GitHub event JSON")?;

    let pr = event["pull_request"].as_object().context(
        "Not a pull_request event — GITHUB_EVENT_PATH does not contain a pull_request payload",
    )?;

    let repo_full_name = event["repository"]["full_name"]
        .as_str()
        .or_else(|| pr.get("head")?.get("repo")?.get("full_name")?.as_str())
        .context("Could not determine repository full name from event payload")?;

    let head_sha = pr["head"]["sha"]
        .as_str()
        .context("Could not determine head SHA from pull_request payload")?;

    let pr_number = pr["number"]
        .as_i64()
        .context("Could not determine PR number from pull_request payload")?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_max_idle_per_host(5)
        .build()
        .context("Failed to build reqwest HTTP client")?;
    let repo_api = format!("https://api.github.com/repos/{repo_full_name}");
    let auth_header = format!("Bearer {token}");

    let files = fetch_pr_files(&client, &repo_api, pr_number, &auth_header)?;

    // Parse every changed file before running detectors so cross-file checks
    // (such as imports versus dependency manifests) have complete context.
    let mut parsed_files = Vec::new();
    let config = crate::config::load(None).unwrap_or_default();
    for file in &files {
        let filename = file["filename"].as_str().unwrap_or("unknown");
        let patch = match file["patch"].as_str() {
            Some(p) if !p.is_empty() && p.len() < 100_000 => p,
            _ => continue,
        };
        if let Ok(parsed) = crate::parser::parse_unified_diff(filename, patch) {
            parsed_files.push(parsed);
        }
    }
    let findings = if parsed_files.is_empty() {
        crate::detectors::Findings::new()
    } else {
        crate::detectors::run_all(&parsed_files, &config)
    };

    // Create the check run in "in_progress" status
    let check_run_url = format!("{repo_api}/check-runs");
    let check_run: serde_json::Value = retry_blocking(
        &RetryConfig::api_default(),
        "create_check_run",
        &is_reqwest_error_retryable,
        || {
            let body = serde_json::json!({
                "name": "codasaurus",
                "head_sha": head_sha,
                "status": "in_progress",
            });
            client
                .post(&check_run_url)
                .header("Authorization", &auth_header)
                .header("Accept", "application/vnd.github+json")
                .header(
                    "User-Agent",
                    concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
                )
                .json(&body)
                .send()
                .context("Failed to create check run")
                .and_then(|r| {
                    r.error_for_status()
                        .context("GitHub rejected check run creation")
                })
                .and_then(|r| {
                    r.json::<serde_json::Value>()
                        .context("Failed to parse check run creation response")
                })
                .map_err(|e| anyhow::anyhow!("{e:#}"))
        },
    )?;

    let check_run_id = check_run["id"]
        .as_i64()
        .context("Check run creation response missing 'id' field")?;

    // Tally severities
    let has_blocking = findings.has_blocking();
    let counts = findings.count_by_severity();
    let blocking = counts.get("blocking").copied().unwrap_or(0);
    let warnings = counts.get("warning").copied().unwrap_or(0);
    let infos = counts.get("info").copied().unwrap_or(0);

    // Build annotations (GitHub API limit: 50 per check run)
    let all_annotations: Vec<serde_json::Value> = findings
        .findings
        .iter()
        .map(|f| {
            serde_json::json!({
                "path": f.file,
                "start_line": f.line.max(1),
                "end_line": f.line.max(1),
                "annotation_level": match f.severity {
                    "blocking" => "failure",
                    "warning" => "warning",
                    _ => "notice",
                },
                "message": format!("[{}] {}", f.detector, f.message),
            })
        })
        .collect();

    let truncated = all_annotations.len() > 50;
    let annotations: Vec<serde_json::Value> = all_annotations.into_iter().take(50).collect();

    // Update the check run with completed status and annotations
    let mut summary = format!("{blocking} blocking, {warnings} warnings, {infos} info");
    if truncated {
        summary.push_str(" (annotations truncated to 50 — GitHub API limit)");
    }

    let update_url = format!("{repo_api}/check-runs/{check_run_id}");
    retry_blocking(
        &RetryConfig::api_default(),
        "update_check_run",
        &is_reqwest_error_retryable,
        || {
            let body = serde_json::json!({
                "status": "completed",
                "conclusion": if has_blocking { "failure" } else { "success" },
                "output": {
                    "title": "Codasaurus Review",
                    "summary": &summary,
                    "annotations": &annotations,
                }
            });
            client
                .patch(&update_url)
                .header("Authorization", &auth_header)
                .header("Accept", "application/vnd.github+json")
                .header(
                    "User-Agent",
                    concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
                )
                .json(&body)
                .send()
                .context("Failed to update check run")
                .and_then(|r| {
                    r.error_for_status()
                        .context("GitHub rejected check run update")
                })
                .map_err(|e| anyhow::anyhow!("{e:#}"))
        },
    )?;

    Ok(())
}

fn fetch_pr_files(
    client: &reqwest::blocking::Client,
    repo_api: &str,
    pr_number: i64,
    auth_header: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut files = Vec::new();

    for page in 1..=MAX_PR_FILE_PAGES {
        let url =
            format!("{repo_api}/pulls/{pr_number}/files?per_page={PR_FILES_PER_PAGE}&page={page}");
        let page_files: Vec<serde_json::Value> = retry_blocking(
            &RetryConfig::api_default(),
            "fetch_pr_files_page",
            &is_reqwest_error_retryable,
            || {
                client
                    .get(&url)
                    .header("Authorization", auth_header)
                    .header("Accept", "application/vnd.github+json")
                    .header(
                        "User-Agent",
                        concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
                    )
                    .send()
                    .context("Failed to fetch PR files from GitHub API")
                    .and_then(|r| {
                        r.error_for_status()
                            .context("GitHub rejected PR-file request")
                    })
                    .and_then(|r| {
                        r.json::<Vec<serde_json::Value>>()
                            .context("Failed to parse PR files response as JSON")
                    })
                    .map_err(|e| anyhow::anyhow!("{e:#}"))
            },
        )?;
        let is_last_page = page_files.len() < PR_FILES_PER_PAGE;
        files.extend(page_files);

        if is_last_page || page == MAX_PR_FILE_PAGES {
            return Ok(files);
        }
    }

    unreachable!("the bounded page loop always returns")
}
