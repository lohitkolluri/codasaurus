//! Build remote repo awareness for PR reviews (manifests, guidelines, CODEOWNERS, issues).

use super::codeowners::{owners_for_paths, parse_codeowners};
use super::github_files::{fetch_first_existing, fetch_repo_files_parallel};
use crate::context::guidelines::GuidelineFile;
use crate::llm::{IssueContext, ReviewContext};
use crate::parser::ParsedFile;
use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use anyhow::Result;
use regex::Regex;
use reqwest::header::HeaderMap;
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};
use tokio::sync::Semaphore;

static ISSUE_REF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:fixes|closes|resolves|fix(?:es)?)\s+#(\d+)\b").expect("issue ref regex")
});

static JIRA_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([A-Z][A-Z0-9]+-\d+)\b").expect("jira key regex"));

static LINEAR_ISSUE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"linear\.app/[^\s]+/issue/([A-Z]+-\d+)").expect("linear issue regex")
});

const GUIDELINE_PATHS: &[&str] = &[
    "CONTRIBUTING.md",
    "AGENTS.md",
    "CLAUDE.md",
    ".github/CONTRIBUTING.md",
    "docs/CONTRIBUTING.md",
];

const CODEOWNERS_PATHS: &[&str] = &["CODEOWNERS", ".github/CODEOWNERS", "docs/CODEOWNERS"];

/// Remote context gathered for one PR review.
#[derive(Debug, Default)]
pub struct RemoteRepoContext {
    pub manifests_added: usize,
    pub guidelines: Vec<GuidelineFile>,
    pub codeowner_reviewers: Vec<String>,
    pub linked_issues: Vec<IssueContext>,
    /// Compact text for LLM / walkthrough.
    pub summary: String,
}

/// Infer which dependency manifests we need from changed file languages/paths.
pub fn needed_manifests(changed_paths: &[String]) -> Vec<&'static str> {
    let mut need = HashSet::new();
    for path in changed_paths {
        let lower = path.to_lowercase();
        let ext = lower.rsplit('.').next().unwrap_or("");
        let base = lower.rsplit('/').next().unwrap_or("");
        if matches!(
            ext,
            "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "vue" | "svelte"
        ) || base == "package.json"
        {
            need.insert("package.json");
        }
        if matches!(ext, "rs") || base == "cargo.toml" {
            need.insert("Cargo.toml");
        }
        if matches!(ext, "py" | "pyi")
            || base == "requirements.txt"
            || base == "pyproject.toml"
            || base == "setup.py"
        {
            need.insert("pyproject.toml");
            need.insert("requirements.txt");
        }
        if matches!(ext, "go") || base == "go.mod" {
            need.insert("go.mod");
        }
    }
    need.into_iter().collect()
}

/// Fetch missing manifests at `git_ref` and parse them into `ParsedFile`s.
/// Also probes nested package manifests next to changed files (monorepos).
pub async fn bootstrap_manifests(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    git_ref: &str,
    changed_paths: &[String],
    already_have: &HashSet<String>,
) -> Result<Vec<ParsedFile>> {
    let mut out = Vec::new();
    let mut attempted = HashSet::new();

    let mut candidates: Vec<String> = needed_manifests(changed_paths)
        .into_iter()
        .map(str::to_string)
        .collect();

    // Nested monorepo: for `packages/foo/src/a.ts` also try `packages/foo/package.json`.
    for path in changed_paths.iter().take(80) {
        if let Some(parent) = path.rsplit_once('/').map(|(p, _)| p) {
            let lower = path.to_lowercase();
            let ext = lower.rsplit('.').next().unwrap_or("");
            if matches!(
                ext,
                "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "vue" | "svelte"
            ) {
                candidates.push(format!("{parent}/package.json"));
            } else if ext == "rs" {
                candidates.push(format!("{parent}/Cargo.toml"));
                // Walk one level up for workspace crates: crate/src/lib.rs → crate/Cargo.toml
                if let Some(grand) = parent.rsplit_once('/').map(|(p, _)| p) {
                    if parent.ends_with("/src") {
                        candidates.push(format!("{grand}/Cargo.toml"));
                    }
                }
            } else if matches!(ext, "py" | "pyi") {
                candidates.push(format!("{parent}/pyproject.toml"));
                candidates.push(format!("{parent}/requirements.txt"));
            } else if ext == "go" {
                candidates.push(format!("{parent}/go.mod"));
            }
        }
    }

    candidates.sort();
    candidates.dedup();
    candidates.truncate(24);

    let mut to_fetch = Vec::new();
    for path in candidates {
        let key = path.to_lowercase();
        if !attempted.insert(key.clone()) {
            continue;
        }
        if already_have
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&path) || p.ends_with(&format!("/{key}")))
        {
            continue;
        }
        to_fetch.push(path);
    }

    let fetched = fetch_repo_files_parallel(client, headers, repo, &to_fetch, git_ref, 5).await;
    for (path, content) in fetched {
        if let Ok(parsed) = crate::parser::parse_file(&path, &content) {
            out.push(parsed);
        }
    }
    Ok(out)
}

/// Fetch guideline markdown files from the repo (up to a few known names).
pub async fn fetch_guidelines(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    git_ref: &str,
) -> Result<Vec<GuidelineFile>> {
    let paths: Vec<String> = GUIDELINE_PATHS.iter().map(|p| (*p).to_string()).collect();
    let fetched = fetch_repo_files_parallel(client, headers, repo, &paths, git_ref, 5).await;

    let mut files = Vec::new();
    for path in GUIDELINE_PATHS {
        if files.len() >= 3 {
            break;
        }
        if let Some((_, content)) = fetched.iter().find(|(p, _)| p == path) {
            if let Some(gf) = GuidelineFile::from_content(*path, path, content.clone()) {
                files.push(gf);
            }
        }
    }
    Ok(files)
}

/// CODEOWNERS → reviewer logins for changed paths.
pub async fn fetch_codeowner_reviewers(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    git_ref: &str,
    changed_paths: &[String],
) -> Result<Vec<String>> {
    let Some((path, content)) =
        fetch_first_existing(client, headers, repo, CODEOWNERS_PATHS, git_ref).await?
    else {
        return Ok(Vec::new());
    };
    let _ = path;
    let rules = parse_codeowners(&content);
    Ok(owners_for_paths(&rules, changed_paths))
}

/// Parse Fixes/Closes/Resolves #N from PR body and fetch issue titles (budgeted).
pub async fn fetch_linked_issues(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    pr_body: &str,
) -> Result<Vec<IssueContext>> {
    let mut numbers: Vec<u64> = ISSUE_REF_RE
        .captures_iter(pr_body)
        .filter_map(|c| c.get(1)?.as_str().parse().ok())
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    numbers.truncate(5);

    if numbers.is_empty() {
        return Ok(Vec::new());
    }

    let semaphore = Arc::new(Semaphore::new(5));
    let mut handles = Vec::with_capacity(numbers.len());
    for n in numbers {
        let cl = client.clone();
        let hdrs = headers.clone();
        let repo = repo.to_string();
        let sem = Arc::clone(&semaphore);
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.ok()?;
            let url = format!("https://api.github.com/repos/{repo}/issues/{n}");
            let resp = retry_async(
                &RetryConfig::api_default(),
                "fetch_linked_issue",
                &is_reqwest_error_retryable,
                || async {
                    cl.get(&url)
                        .headers(hdrs.clone())
                        .send()
                        .await
                        .map_err(Into::into)
                },
            )
            .await
            .ok()?;
            if !resp.status().is_success() {
                return None;
            }
            let body = resp.json::<serde_json::Value>().await.ok()?;
            // Skip if it's actually a PR
            if body.get("pull_request").is_some() {
                return None;
            }
            let title = body["title"].as_str().unwrap_or("").to_string();
            let issue_body = body["body"].as_str().map(|s| {
                let t: String = s.chars().take(800).collect();
                t
            });
            Some(IssueContext {
                number: n,
                title,
                body: issue_body,
            })
        }));
    }

    let mut issues = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(Some(issue)) = h.await {
            issues.push(issue);
        }
    }
    issues.sort_by_key(|i| i.number);
    Ok(issues)
}

/// Flatten Atlassian Document Format (ADF) or plain-string Jira descriptions.
pub fn jira_description_text(value: &serde_json::Value) -> Option<String> {
    if value.is_null() {
        return None;
    }
    if let Some(s) = value.as_str() {
        let t = s.trim();
        return if t.is_empty() {
            None
        } else {
            Some(t.chars().take(800).collect())
        };
    }
    let mut out = String::new();
    fn walk(node: &serde_json::Value, out: &mut String) {
        if let Some(t) = node.get("text").and_then(|v| v.as_str()) {
            if !out.is_empty() && !out.ends_with([' ', '\n']) {
                out.push(' ');
            }
            out.push_str(t);
        }
        if let Some(arr) = node.get("content").and_then(|v| v.as_array()) {
            for (i, child) in arr.iter().enumerate() {
                let before = out.len();
                walk(child, out);
                let ty = child.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if matches!(
                    ty,
                    "paragraph" | "heading" | "bulletList" | "orderedList" | "listItem"
                ) && out.len() > before
                    && !out.ends_with('\n')
                {
                    out.push('\n');
                }
                if i + 1 < arr.len() && ty == "hardBreak" {
                    out.push('\n');
                }
            }
        }
    }
    walk(value, &mut out);
    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(800).collect())
    }
}

fn ticket_number(key: &str) -> u64 {
    key.split('-')
        .next_back()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn unique_keys(haystack: &str, re: &Regex, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for caps in re.captures_iter(haystack) {
        let Some(m) = caps.get(1) else { continue };
        let key = m.as_str().to_string();
        if seen.insert(key.clone()) {
            out.push(key);
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

/// Parse Jira keys (`PROJ-123`) and Linear URLs/ids from PR title+body; fetch when credentials exist.
pub async fn fetch_external_tickets(pr_title: &str, pr_body: &str) -> Vec<IssueContext> {
    let haystack = format!("{pr_title}\n{pr_body}");
    let mut out = Vec::new();

    let keys = unique_keys(&haystack, &JIRA_KEY_RE, 5);
    let linear_from_url = unique_keys(&haystack, &LINEAR_ISSUE_RE, 5);
    let mut linear_ids = linear_from_url.clone();
    // Bare PROJ-123 also works as a Linear identifier when Linear is configured.
    for k in &keys {
        if !linear_ids.iter().any(|id| id == k) {
            linear_ids.push(k.clone());
        }
    }
    linear_ids.truncate(5);

    let jira_configured = std::env::var("JIRA_BASE_URL").is_ok()
        && std::env::var("JIRA_EMAIL").is_ok()
        && std::env::var("JIRA_API_TOKEN").is_ok();
    let linear_configured = std::env::var("LINEAR_API_KEY").is_ok();

    if let (Ok(base), Ok(email), Ok(token)) = (
        std::env::var("JIRA_BASE_URL"),
        std::env::var("JIRA_EMAIL"),
        std::env::var("JIRA_API_TOKEN"),
    ) {
        if crate::ssrf::validate_http_url(base.trim(), false).is_ok() {
            let client = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
            {
                Ok(c) => c,
                Err(_) => reqwest::Client::new(),
            };
            for key in &keys {
                let url = format!("{}/rest/api/3/issue/{}", base.trim_end_matches('/'), key);
                if let Ok(resp) = client
                    .get(&url)
                    .basic_auth(&email, Some(&token))
                    .header("Accept", "application/json")
                    .send()
                    .await
                {
                    if resp.status().is_success() {
                        if let Ok(v) = resp.json::<serde_json::Value>().await {
                            let title = v["fields"]["summary"].as_str().unwrap_or(key).to_string();
                            let body = jira_description_text(&v["fields"]["description"]);
                            out.push(IssueContext {
                                number: ticket_number(key),
                                title: format!("[Jira {key}] {title}"),
                                body,
                            });
                        }
                    }
                }
            }
        }
    } else if !keys.is_empty() && !jira_configured && !linear_configured {
        // Don't stub tickets into the walkthrough — a "configure …" placeholder
        // looks like a linked issue and scores as "unclear". Operators enable
        // Jira/Linear under Settings → Connections when they want this context.
        tracing::debug!(
            count = keys.len(),
            "ticket keys in PR but Jira/Linear not configured; skipping"
        );
    }

    if let Ok(api_key) = std::env::var("LINEAR_API_KEY") {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
        {
            Ok(c) => c,
            Err(_) => reqwest::Client::new(),
        };
        for id in &linear_ids {
            let from_url = linear_from_url.iter().any(|u| u == id);
            // `issue(id:)` accepts Linear identifiers like ENG-123 (and UUIDs).
            let query = serde_json::json!({
                "query": "query($id: String!) { issue(id: $id) { identifier title description } }",
                "variables": { "id": id }
            });
            match client
                .post("https://api.linear.app/graphql")
                .header("Authorization", api_key.as_str())
                .header("Content-Type", "application/json")
                .json(&query)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(v) = resp.json::<serde_json::Value>().await {
                        let node = &v["data"]["issue"];
                        if node.is_object() && !node.is_null() {
                            let title = node["title"].as_str().unwrap_or(id).to_string();
                            let body = node["description"]
                                .as_str()
                                .map(|s| s.chars().take(800).collect());
                            out.push(IssueContext {
                                number: ticket_number(id),
                                title: format!("[Linear {id}] {title}"),
                                body,
                            });
                            continue;
                        }
                    }
                    // Bare keys may be Jira-only — only surface misses for explicit Linear URLs.
                    if from_url {
                        out.push(IssueContext {
                            number: 0,
                            title: format!("[Linear {id}]"),
                            body: Some("Linked from PR (fetch returned no issue).".into()),
                        });
                    }
                }
                _ if from_url => {
                    out.push(IssueContext {
                        number: 0,
                        title: format!("[Linear {id}]"),
                        body: Some("Linked from PR (Linear fetch failed).".into()),
                    });
                }
                _ => {}
            }
        }
    }
    out
}

/// Assemble remote awareness used by detectors + LLM.
#[allow(clippy::too_many_arguments)]
pub async fn gather_remote_context(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    base_sha: &str,
    head_ref: &str,
    pr_title: &str,
    pr_body: &str,
    changed_paths: &[String],
    already_have_paths: &HashSet<String>,
) -> Result<(RemoteRepoContext, Vec<ParsedFile>)> {
    let git_ref = if base_sha.is_empty() {
        "HEAD"
    } else {
        base_sha
    };
    let head_ref = if head_ref.is_empty() {
        git_ref
    } else {
        head_ref
    };

    let (manifests_res, guidelines_res, codeowners_res, issues_res) = tokio::join!(
        bootstrap_manifests(
            client,
            headers,
            repo,
            git_ref,
            changed_paths,
            already_have_paths
        ),
        fetch_guidelines(client, headers, repo, head_ref),
        fetch_codeowner_reviewers(client, headers, repo, git_ref, changed_paths),
        fetch_linked_issues(client, headers, repo, pr_body),
    );

    let manifests = manifests_res.unwrap_or_default();
    let mut guidelines = guidelines_res.unwrap_or_default();
    if guidelines.is_empty() && head_ref != git_ref {
        guidelines = fetch_guidelines(client, headers, repo, git_ref)
            .await
            .unwrap_or_default();
    }
    let codeowner_reviewers = codeowners_res.unwrap_or_default();
    let linked_issues = {
        let mut issues = issues_res.unwrap_or_default();
        issues.extend(fetch_external_tickets(pr_title, pr_body).await);
        issues
    };

    let mut summary = String::new();
    let _ = std::fmt::Write::write_fmt(
        &mut summary,
        format_args!(
            "Repository: {repo}\nBranch: {head_ref}\nPR: {pr_title}\nChanged files: {}\n",
            changed_paths.len()
        ),
    );
    if !manifests.is_empty() {
        let _ = std::fmt::Write::write_fmt(
            &mut summary,
            format_args!(
                "Bootstrapped manifests: {}\n",
                manifests
                    .iter()
                    .map(|m| m.path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    if !guidelines.is_empty() {
        let _ = std::fmt::Write::write_fmt(
            &mut summary,
            format_args!(
                "Guidelines: {}\n",
                guidelines
                    .iter()
                    .map(|g| g.source.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
        let snippet: String = guidelines
            .iter()
            .flat_map(|g| g.content.chars())
            .take(1_500)
            .collect();
        if !snippet.is_empty() {
            summary.push_str("Guidelines excerpt:\n");
            summary.push_str(&snippet);
            summary.push('\n');
        }
    }
    if !codeowner_reviewers.is_empty() {
        let _ = std::fmt::Write::write_fmt(
            &mut summary,
            format_args!("CODEOWNERS: {}\n", codeowner_reviewers.join(", ")),
        );
    }
    if !linked_issues.is_empty() {
        summary.push_str("Linked issues:\n");
        for iss in &linked_issues {
            let _ = std::fmt::Write::write_fmt(
                &mut summary,
                format_args!("  #{} {}\n", iss.number, iss.title),
            );
        }
    }

    let ctx = RemoteRepoContext {
        manifests_added: manifests.len(),
        guidelines,
        codeowner_reviewers,
        linked_issues,
        summary,
    };
    Ok((ctx, manifests))
}

pub fn to_review_context(
    remote: &RemoteRepoContext,
    repo: &str,
    branch: &str,
    pr_title: &str,
    pr_body: &str,
) -> ReviewContext {
    ReviewContext {
        repo: Some(repo.to_string()),
        branch: Some(branch.to_string()),
        pr_title: Some(pr_title.to_string()),
        pr_description: Some(pr_body.chars().take(2_000).collect()),
        linked_issues: remote.linked_issues.clone(),
        related_prs: Vec::new(),
        repo_context: Some(remote.summary.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needed_manifests_from_langs() {
        let m = needed_manifests(&["src/app.ts".into(), "lib.rs".into()]);
        assert!(m.contains(&"package.json"));
        assert!(m.contains(&"Cargo.toml"));
    }

    #[test]
    fn parses_issue_refs() {
        let body = "This Fixes #42 and closes #7. Also resolves #42 again.";
        let nums: Vec<u64> = ISSUE_REF_RE
            .captures_iter(body)
            .filter_map(|c| c.get(1)?.as_str().parse().ok())
            .collect();
        assert!(nums.contains(&42));
        assert!(nums.contains(&7));
    }

    #[test]
    fn jira_description_plain_string() {
        let v = serde_json::json!("Hello ticket\n\nDetails here");
        assert_eq!(
            jira_description_text(&v).as_deref(),
            Some("Hello ticket\n\nDetails here")
        );
    }

    #[test]
    fn jira_description_adf_paragraphs() {
        let v = serde_json::json!({
            "type": "doc",
            "content": [
                {
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": "First line" }]
                },
                {
                    "type": "paragraph",
                    "content": [
                        { "type": "text", "text": "Second" },
                        { "type": "text", "text": "line" }
                    ]
                }
            ]
        });
        let text = jira_description_text(&v).expect("adf text");
        assert!(text.contains("First line"));
        assert!(text.contains("Second"));
        assert!(text.contains("line"));
    }

    #[test]
    fn jira_description_null_empty() {
        assert!(jira_description_text(&serde_json::Value::Null).is_none());
        assert!(jira_description_text(&serde_json::json!("")).is_none());
        assert!(
            jira_description_text(&serde_json::json!({ "type": "doc", "content": [] })).is_none()
        );
    }

    #[test]
    fn extracts_linear_ids_from_urls() {
        let hay = "See https://linear.app/acme/issue/ENG-99/title and ENG-100";
        let ids = unique_keys(hay, &LINEAR_ISSUE_RE, 5);
        assert_eq!(ids, vec!["ENG-99".to_string()]);
        let keys = unique_keys(hay, &JIRA_KEY_RE, 5);
        assert!(keys.contains(&"ENG-99".to_string()));
        assert!(keys.contains(&"ENG-100".to_string()));
    }
}
