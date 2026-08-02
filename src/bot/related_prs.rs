//! Related PR discovery via path history (budgeted GitHub calls).
//!
//! Algorithm (kept within a small GitHub API budget):
//! 1. **Score** changed paths — prefer source / critical files; deprioritize
//!    meta churn (`CHANGELOG.md`, docs, lockfiles, root markdown).
//! 2. **Sample** up to [`MAX_PATHS`] with root diversity (not raw API order).
//! 3. For each path: recent commits → linked PRs (GitHub's practical approach
//!    when PR search isn't path-indexed).
//! 4. **Rank** by overlap strength (how many sampled paths hit the PR), then
//!    path weight, then newer PR number — not "first path wins."

use crate::llm::is_low_signal_path;
use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use anyhow::Result;
use reqwest::header::HeaderMap;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use tokio::sync::Semaphore;

const USER_AGENT: &str = concat!("codasaurus/", env!("CARGO_PKG_VERSION"));
const MAX_PATHS: usize = 5;
const MAX_COMMITS_PER_PATH: usize = 4;
const MAX_RELATED: usize = 3;
/// Max concurrent GitHub GETs for commit→pulls fan-out (no `futures` dep).
const MAX_CONCURRENT_FETCHES: usize = 4;

#[derive(Debug, Clone)]
struct RelatedCandidate {
    number: i64,
    title: String,
    /// Distinct sampled paths that linked to this PR.
    path_hits: u32,
    /// Sum of path signal scores for those hits.
    weight: i32,
}

/// Find recent PRs that touched the same paths (for LLM context / walkthrough).
pub async fn find_related_prs(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    changed_paths: &[String],
    current_pr: i64,
) -> Result<Vec<String>> {
    let selected = select_paths_for_related(changed_paths, MAX_PATHS);
    if selected.is_empty() {
        return Ok(Vec::new());
    }

    let path_scores: HashMap<String, i32> = selected
        .iter()
        .map(|p| (p.clone(), path_signal_score(p)))
        .collect();

    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_FETCHES));
    let mut path_handles = Vec::with_capacity(selected.len());

    for path in selected {
        let client = client.clone();
        let headers = headers.clone();
        let repo = repo.to_string();
        let sem = sem.clone();
        path_handles.push(tokio::spawn(async move {
            let hits =
                collect_related_for_path(&client, &headers, &repo, &path, current_pr, sem).await;
            (path, hits)
        }));
    }

    // Aggregate across all paths before ranking (no early exit — first path must not dominate).
    let mut by_pr: BTreeMap<i64, RelatedCandidate> = BTreeMap::new();
    for handle in path_handles {
        let Ok((path, items)) = handle.await else {
            continue;
        };
        let score = *path_scores.get(&path).unwrap_or(&1);
        let mut seen_on_path: BTreeSet<i64> = BTreeSet::new();
        for (number, title) in items {
            if !seen_on_path.insert(number) {
                continue;
            }
            by_pr
                .entry(number)
                .and_modify(|c| {
                    c.path_hits = c.path_hits.saturating_add(1);
                    c.weight = c.weight.saturating_add(score);
                    if c.title.is_empty() && !title.is_empty() {
                        c.title = title.clone();
                    }
                })
                .or_insert(RelatedCandidate {
                    number,
                    title,
                    path_hits: 1,
                    weight: score,
                });
        }
    }

    Ok(format_ranked(by_pr.into_values().collect()))
}

/// Pick the most informative paths for history lookup.
fn select_paths_for_related(changed_paths: &[String], max: usize) -> Vec<String> {
    if max == 0 {
        return Vec::new();
    }

    let mut scored: Vec<(i32, String)> = changed_paths
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| (path_signal_score(p), p.clone()))
        .filter(|(s, _)| *s > 0)
        .collect();

    // Higher score first; stable tie-break on path for determinism.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.dedup_by(|a, b| a.1 == b.1);

    let mut selected: Vec<String> = Vec::with_capacity(max.min(scored.len()));
    let mut used_roots: BTreeSet<String> = BTreeSet::new();

    // Pass 1: one best high-signal path per top-level root (skip pure meta).
    for (score, path) in &scored {
        if selected.len() >= max {
            break;
        }
        if *score <= META_SCORE {
            continue;
        }
        let root = top_level_root(path);
        if used_roots.insert(root.to_string()) {
            selected.push(path.clone());
        }
    }

    // Pass 2: fill remaining slots by score (same-root source files, then meta).
    for (_score, path) in &scored {
        if selected.len() >= max {
            break;
        }
        if !selected.iter().any(|s| s == path) {
            selected.push(path.clone());
        }
    }

    selected
}

/// Score for paths that almost every PR touches — usable only as fallback.
const META_SCORE: i32 = 1;

/// How useful is this path for finding *meaningfully* related PRs?
fn path_signal_score(path: &str) -> i32 {
    if path.is_empty() || is_low_signal_path(path) {
        return 0;
    }

    let lower = path.to_ascii_lowercase().replace('\\', "/");
    let file = lower.rsplit('/').next().unwrap_or(lower.as_str());

    if is_meta_churn_path(&lower, file) {
        return META_SCORE;
    }

    let mut score: i32 = 10;

    if lower.starts_with("src/")
        || lower.starts_with("lib/")
        || lower.starts_with("app/")
        || lower.starts_with("packages/")
        || lower.starts_with("crates/")
        || lower.starts_with("svelte-dashboard/src/")
        || lower.starts_with("frontend/")
        || lower.starts_with("backend/")
    {
        score += 20;
    }

    if looks_security_sensitive(path) {
        score += 15;
    }

    let depth = lower.matches('/').count();
    score += (depth.min(4) as i32) * 2;

    let ext = file.rsplit('.').next().unwrap_or("");
    if matches!(
        ext,
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "py"
            | "go"
            | "java"
            | "kt"
            | "rb"
            | "php"
            | "c"
            | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "svelte"
            | "vue"
            | "cs"
            | "swift"
    ) {
        score += 8;
    }

    if lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || file.contains("_test.")
        || file.ends_with(".test.ts")
        || file.ends_with(".test.js")
        || file.ends_with(".spec.ts")
        || file.ends_with(".spec.js")
        || lower.ends_with(".md")
    {
        score -= 8;
    }

    score.max(1)
}

fn is_meta_churn_path(lower: &str, file: &str) -> bool {
    if lower.starts_with("docs/") || lower.starts_with(".github/") {
        return true;
    }
    // Root-level markdown / config almost every PR touches.
    let depth = lower.matches('/').count();
    if depth == 0 && (file.ends_with(".md") || file.starts_with('.')) {
        return true;
    }
    matches!(
        file,
        "changelog.md"
            | "readme.md"
            | "license"
            | "license.md"
            | "licence"
            | "licence.md"
            | "contributing.md"
            | "code_of_conduct.md"
            | "security.md"
            | "authors"
            | "notice"
            | ".gitignore"
            | ".dockerignore"
            | ".editorconfig"
            | ".gitattributes"
            | "dockerfile"
            | "makefile"
            | ".env"
            | ".env.example"
            | ".env.sample"
            | "cargo.toml"
            | "package.json"
            | "pnpm-workspace.yaml"
            | "go.mod"
            | "pyproject.toml"
            | "gemfile"
            | "composer.json"
    )
}

fn top_level_root(path: &str) -> &str {
    path.split('/').next().unwrap_or(path)
}

/// Prefer auth / crypto / infra paths when ranking samples (local copy to avoid
/// coupling to the review pipeline module tree).
fn looks_security_sensitive(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("auth")
        || lower.contains("crypto")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("middleware")
        || lower.contains("rbac")
        || lower.contains("permission")
        || lower.contains("/k8s/")
        || lower.contains("/kubernetes/")
        || lower.contains("/helm/")
        || lower.ends_with(".tf")
        || lower.ends_with(".tfvars")
}

async fn collect_related_for_path(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    path: &str,
    current_pr: i64,
    sem: Arc<Semaphore>,
) -> Vec<(i64, String)> {
    let mut out = Vec::new();
    let encoded = encode_path_query(path);
    let url = format!(
        "https://api.github.com/repos/{repo}/commits?path={encoded}&per_page={MAX_COMMITS_PER_PATH}"
    );

    let _permit = match sem.acquire().await {
        Ok(p) => p,
        Err(_) => return out,
    };
    let commits = match fetch_json_array(client, headers, &url).await {
        Ok(v) => v,
        Err(_) => return out,
    };
    drop(_permit);

    let mut seen_shas = BTreeSet::new();
    let mut pull_handles = Vec::new();

    for c in commits {
        let sha = c["sha"].as_str().unwrap_or("").to_string();
        if sha.is_empty() || !seen_shas.insert(sha.clone()) {
            continue;
        }
        let msg = c["commit"]["message"].as_str().unwrap_or("").to_string();
        let client = client.clone();
        let headers = headers.clone();
        let repo = repo.to_string();
        let sem = sem.clone();
        pull_handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.ok()?;
            let pulls_url = format!("https://api.github.com/repos/{repo}/commits/{sha}/pulls");
            match fetch_json_array(&client, &headers, &pulls_url).await {
                Ok(pulls) if !pulls.is_empty() => {
                    let mut items = Vec::new();
                    for p in pulls {
                        let n = p["number"].as_i64().unwrap_or(0);
                        if n <= 0 || n == current_pr {
                            continue;
                        }
                        let title = p["title"].as_str().unwrap_or("").to_string();
                        items.push((n, title));
                    }
                    Some(items)
                }
                _ => {
                    // Fallback: parse `(#123)` / `#123` from commit subject.
                    let mut items = Vec::new();
                    for n in extract_pr_numbers(&msg) {
                        if n as i64 == current_pr {
                            continue;
                        }
                        items.push((n as i64, format!("from commit {sha}")));
                    }
                    Some(items)
                }
            }
        }));
    }

    for handle in pull_handles {
        if let Ok(Some(items)) = handle.await {
            out.extend(items);
        }
    }
    out
}

fn format_ranked(mut candidates: Vec<RelatedCandidate>) -> Vec<String> {
    candidates.sort_by(|a, b| {
        b.path_hits
            .cmp(&a.path_hits)
            .then_with(|| b.weight.cmp(&a.weight))
            .then_with(|| b.number.cmp(&a.number))
    });
    candidates
        .into_iter()
        .take(MAX_RELATED)
        .map(|c| {
            let title: String = c.title.chars().take(72).collect();
            // Prefer `#N` alone for the number so GitHub can auto-link without
            // repeating the PR title (GitHub UI already shows it on hover).
            if title.is_empty() {
                if c.path_hits > 1 {
                    format!("#{} ({} shared files)", c.number, c.path_hits)
                } else {
                    format!("#{}", c.number)
                }
            } else if c.path_hits > 1 {
                format!("#{}: {title} ({} shared files)", c.number, c.path_hits)
            } else {
                format!("#{}: {title}", c.number)
            }
        })
        .collect()
}

fn encode_path_query(path: &str) -> String {
    let mut out = String::new();
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn fetch_json_array(
    client: &reqwest::Client,
    headers: &HeaderMap,
    url: &str,
) -> Result<Vec<serde_json::Value>> {
    let resp = retry_async(
        &RetryConfig::quick(),
        "related_prs_fetch",
        &is_reqwest_error_retryable,
        || async {
            client
                .get(url)
                .headers(headers.clone())
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/vnd.github+json")
                .send()
                .await
                .map_err(Into::into)
        },
    )
    .await?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    Ok(resp.json().await.unwrap_or_default())
}

fn extract_pr_numbers(msg: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let bytes = msg.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let mut j = i + 1;
            let mut n: u64 = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                n = n
                    .saturating_mul(10)
                    .saturating_add((bytes[j] - b'0') as u64);
                j += 1;
            }
            if j > i + 1 && n > 0 {
                out.push(n);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_hashes() {
        let nums = extract_pr_numbers("Fix bug (#42) and #7");
        assert!(nums.contains(&42));
        assert!(nums.contains(&7));
    }

    #[test]
    fn meta_paths_score_lowest() {
        assert_eq!(path_signal_score("CHANGELOG.md"), META_SCORE);
        assert_eq!(path_signal_score(".dockerignore"), META_SCORE);
        assert_eq!(path_signal_score(".env.example"), META_SCORE);
        assert_eq!(path_signal_score("docs/setup.md"), META_SCORE);
        assert_eq!(path_signal_score("package-lock.json"), 0);
    }

    #[test]
    fn source_paths_outrank_meta() {
        let src = path_signal_score("src/bot/related_prs.rs");
        let dash = path_signal_score("svelte-dashboard/src/pages/app/Settings.svelte");
        let meta = path_signal_score("CHANGELOG.md");
        assert!(src > meta);
        assert!(dash > meta);
        assert!(src > path_signal_score("README.md"));
    }

    #[test]
    fn select_skips_meta_when_source_available() {
        let paths = vec![
            ".dockerignore".into(),
            ".env.example".into(),
            "CHANGELOG.md".into(),
            "src/bot/mod.rs".into(),
            "src/main.rs".into(),
            "svelte-dashboard/src/App.svelte".into(),
        ];
        let selected = select_paths_for_related(&paths, 3);
        assert_eq!(selected.len(), 3);
        assert!(selected
            .iter()
            .all(|p| !p.ends_with(".md") && !p.starts_with('.')));
        assert!(selected.iter().any(|p| p.contains("src/")));
    }

    #[test]
    fn select_falls_back_to_meta_when_only_meta() {
        let paths = vec!["CHANGELOG.md".into(), ".dockerignore".into()];
        let selected = select_paths_for_related(&paths, 2);
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn ranks_by_overlap_not_pr_number() {
        let ranked = format_ranked(vec![
            RelatedCandidate {
                number: 8,
                title: "newer but one path".into(),
                path_hits: 1,
                weight: 10,
            },
            RelatedCandidate {
                number: 5,
                title: "older but three paths".into(),
                path_hits: 3,
                weight: 40,
            },
            RelatedCandidate {
                number: 7,
                title: "two paths".into(),
                path_hits: 2,
                weight: 25,
            },
        ]);
        assert!(ranked[0].starts_with("#5"));
        assert!(ranked[0].contains("3 shared files"));
        assert!(ranked[1].starts_with("#7"));
        assert!(ranked[2].starts_with("#8"));
        assert!(!ranked[2].contains("shared files"));
    }
}
