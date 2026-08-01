//! Related PR discovery via path history (budgeted GitHub calls).

use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use anyhow::Result;
use reqwest::header::HeaderMap;
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::Semaphore;

const USER_AGENT: &str = concat!("codasaurus/", env!("CARGO_PKG_VERSION"));
const MAX_PATHS: usize = 3;
const MAX_COMMITS_PER_PATH: usize = 4;
const MAX_RELATED: usize = 5;
/// Max concurrent GitHub GETs for commit→pulls fan-out (no `futures` dep).
const MAX_CONCURRENT_FETCHES: usize = 4;

/// Find recent PRs that touched the same paths (for LLM context / walkthrough).
pub async fn find_related_prs(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    changed_paths: &[String],
    current_pr: i64,
) -> Result<Vec<String>> {
    let paths: Vec<String> = changed_paths
        .iter()
        .filter(|p| !p.is_empty())
        .take(MAX_PATHS)
        .cloned()
        .collect();

    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_FETCHES));
    let mut path_handles = Vec::with_capacity(paths.len());

    for path in paths {
        let client = client.clone();
        let headers = headers.clone();
        let repo = repo.to_string();
        let sem = sem.clone();
        path_handles.push(tokio::spawn(async move {
            collect_related_for_path(&client, &headers, &repo, &path, current_pr, sem).await
        }));
    }

    let mut related: BTreeSet<(i64, String)> = BTreeSet::new();
    for handle in path_handles {
        let Ok(items) = handle.await else {
            continue;
        };
        for item in items {
            related.insert(item);
            if related.len() >= MAX_RELATED {
                return Ok(format_related(&related));
            }
        }
    }

    Ok(format_related(&related))
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

fn format_related(related: &BTreeSet<(i64, String)>) -> Vec<String> {
    related
        .iter()
        .rev() // newer-ish numbers last in BTree — reverse for higher first
        .take(MAX_RELATED)
        .map(|(n, title)| {
            if title.is_empty() {
                format!("#{n}")
            } else {
                let t: String = title.chars().take(80).collect();
                format!("#{n} {t}")
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
}
