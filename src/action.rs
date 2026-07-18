use anyhow::{Context, Result};

/// Run codasaurus as a GitHub Action, posting findings as a Check Run with annotations.
///
/// Reads the GitHub event payload from `GITHUB_EVENT_PATH`, fetches the PR diff,
/// runs detectors on the changed files, and creates/updates a check run with
/// per-finding annotations (capped at 50 per the GitHub API limit).
pub fn run_check_run(event_path: Option<String>) -> Result<()> {
    let event_path = event_path
        .or_else(|| std::env::var("GITHUB_EVENT_PATH").ok())
        .context("No event path provided and GITHUB_EVENT_PATH not set")?;

    let token = std::env::var("GITHUB_TOKEN")
        .context("GITHUB_TOKEN environment variable not set")?;

    let event_json =
        std::fs::read_to_string(&event_path).context("Failed to read GITHUB_EVENT_PATH file")?;

    let event: serde_json::Value =
        serde_json::from_str(&event_json).context("Failed to parse GitHub event JSON")?;

    let pr = event["pull_request"]
        .as_object()
        .context("Not a pull_request event — GITHUB_EVENT_PATH does not contain a pull_request payload")?;

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

    let client = reqwest::blocking::Client::new();
    let repo_api = format!("https://api.github.com/repos/{}", repo_full_name);
    let auth_header = format!("Bearer {}", token);

    // Fetch the list of changed files in the PR
    let files_text: String = client
        .get(format!("{}/pulls/{}/files", repo_api, pr_number))
        .header("Authorization", &auth_header)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus/0.1.0")
        .send()
        .context("Failed to fetch PR files from GitHub API")?
        .text()
        .context("Failed to read PR files response body")?;

    let files: Vec<serde_json::Value> = serde_json::from_str(&files_text)
        .context("Failed to parse PR files response as JSON")?;

    // Run detectors on each changed file's patch
    let mut findings = crate::detectors::Findings::new();
    let config = crate::config::Config::default();
    for file in &files {
        let filename = file["filename"].as_str().unwrap_or("unknown");
        let patch = match file["patch"].as_str() {
            Some(p) if !p.is_empty() && p.len() < 100_000 => p,
            _ => continue,
        };
        if let Ok(parsed) = crate::parser::parse_file(filename, patch) {
            findings.extend(
                crate::detectors::run_all(&[parsed], &config).findings,
            );
        }
    }

    // Create the check run in "in_progress" status
    let check_run: serde_json::Value = client
        .post(format!("{}/check-runs", repo_api))
        .header("Authorization", &auth_header)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus/0.1.0")
        .json(&serde_json::json!({
            "name": "codasaurus",
            "head_sha": head_sha,
            "status": "in_progress",
        }))
        .send()
        .context("Failed to create check run")?
        .json()
        .context("Failed to parse check run creation response")?;

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
    let annotations: Vec<serde_json::Value> =
        all_annotations.into_iter().take(50).collect();

    // Update the check run with completed status and annotations
    let mut summary = format!(
        "{} blocking, {} warnings, {} info",
        blocking, warnings, infos
    );
    if truncated {
        summary.push_str(" (annotations truncated to 50 — GitHub API limit)");
    }

    client
        .patch(format!(
            "{}/check-runs/{}",
            repo_api, check_run_id
        ))
        .header("Authorization", &auth_header)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus/0.1.0")
        .json(&serde_json::json!({
            "status": "completed",
            "conclusion": if has_blocking { "failure" } else { "success" },
            "output": {
                "title": "Codasaurus Review",
                "summary": summary,
                "annotations": annotations,
            }
        }))
        .send()
        .context("Failed to update check run")?;

    Ok(())
}
