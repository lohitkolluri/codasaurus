//! Build remote repo awareness for PR reviews (manifests, guidelines, CODEOWNERS, issues).

use super::codeowners::{owners_for_paths, parse_codeowners};
use super::github_files::{fetch_first_existing, fetch_repo_file};
use crate::context::guidelines::GuidelineFile;
use crate::llm::{IssueContext, ReviewContext};
use crate::parser::ParsedFile;
use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use anyhow::Result;
use regex::Regex;
use reqwest::header::HeaderMap;
use std::collections::HashSet;
use std::sync::LazyLock;

static ISSUE_REF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:fixes|closes|resolves|fix(?:es)?)\s+#(\d+)\b")
        .expect("issue ref regex")
});

const GUIDELINE_PATHS: &[&str] = &[
    "CONTRIBUTING.md",
    "AGENTS.md",
    "CLAUDE.md",
    ".github/CONTRIBUTING.md",
    "docs/CONTRIBUTING.md",
];

const CODEOWNERS_PATHS: &[&str] = &[
    "CODEOWNERS",
    ".github/CODEOWNERS",
    "docs/CODEOWNERS",
];

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
        if let Some(content) = fetch_repo_file(client, headers, repo, &path, git_ref).await? {
            if let Ok(parsed) = crate::parser::parse_file(&path, &content) {
                out.push(parsed);
            }
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
    let mut files = Vec::new();
    for path in GUIDELINE_PATHS {
        if files.len() >= 3 {
            break;
        }
        if let Some(content) = fetch_repo_file(client, headers, repo, path, git_ref).await? {
            if let Some(gf) = GuidelineFile::from_content(*path, path, content) {
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

    let mut issues = Vec::new();
    for n in numbers {
        let url = format!("https://api.github.com/repos/{repo}/issues/{n}");
        let resp = retry_async(
            &RetryConfig::api_default(),
            "fetch_linked_issue",
            &is_reqwest_error_retryable,
            || async {
                client
                    .get(&url)
                    .headers(headers.clone())
                    .send()
                    .await
                    .map_err(Into::into)
            },
        )
        .await;
        let Ok(resp) = resp else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(body) = resp.json::<serde_json::Value>().await else {
            continue;
        };
        // Skip if it's actually a PR
        if body.get("pull_request").is_some() {
            continue;
        }
        let title = body["title"].as_str().unwrap_or("").to_string();
        let issue_body = body["body"].as_str().map(|s| {
            let t: String = s.chars().take(800).collect();
            t
        });
        issues.push(IssueContext {
            number: n,
            title,
            body: issue_body,
        });
    }
    Ok(issues)
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
    let git_ref = if base_sha.is_empty() { "HEAD" } else { base_sha };
    let head_ref = if head_ref.is_empty() { git_ref } else { head_ref };

    let (manifests_res, guidelines_res, codeowners_res, issues_res) = tokio::join!(
        bootstrap_manifests(client, headers, repo, git_ref, changed_paths, already_have_paths),
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
    let linked_issues = issues_res.unwrap_or_default();

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
}
