//! Related PR discovery via path history (budgeted GitHub calls).

use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use anyhow::Result;
use reqwest::header::HeaderMap;
use std::collections::BTreeSet;

const USER_AGENT: &str = concat!("codasaurus/", env!("CARGO_PKG_VERSION"));
const MAX_PATHS: usize = 3;
const MAX_COMMITS_PER_PATH: usize = 4;
const MAX_RELATED: usize = 5;

/// Find recent PRs that touched the same paths (for LLM context / walkthrough).
pub async fn find_related_prs(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    changed_paths: &[String],
    current_pr: i64,
) -> Result<Vec<String>> {
    let mut related: BTreeSet<(i64, String)> = BTreeSet::new();
    let mut seen_shas = BTreeSet::new();

    for path in changed_paths.iter().filter(|p| !p.is_empty()).take(MAX_PATHS) {
        let encoded = encode_path_query(path);
        let url = format!(
            "https://api.github.com/repos/{repo}/commits?path={encoded}&per_page={MAX_COMMITS_PER_PATH}"
        );
        let commits = match fetch_json_array(client, headers, &url).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        for c in commits {
            let sha = c["sha"].as_str().unwrap_or("");
            if sha.is_empty() || !seen_shas.insert(sha.to_string()) {
                continue;
            }
            // Prefer linked PR list for the commit.
            let pulls_url = format!("https://api.github.com/repos/{repo}/commits/{sha}/pulls");
            if let Ok(pulls) = fetch_json_array(client, headers, &pulls_url).await {
                for p in pulls {
                    let n = p["number"].as_i64().unwrap_or(0);
                    if n <= 0 || n == current_pr {
                        continue;
                    }
                    let title = p["title"].as_str().unwrap_or("").to_string();
                    related.insert((n, title));
                    if related.len() >= MAX_RELATED {
                        return Ok(format_related(&related));
                    }
                }
            } else if let Some(msg) = c["commit"]["message"].as_str() {
                // Fallback: parse `(#123)` / `#123` from commit subject.
                for n in extract_pr_numbers(msg) {
                    if n as i64 == current_pr {
                        continue;
                    }
                    related.insert((n as i64, format!("from commit {sha}")));
                    if related.len() >= MAX_RELATED {
                        return Ok(format_related(&related));
                    }
                }
            }
        }
    }

    Ok(format_related(&related))
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
                n = n.saturating_mul(10).saturating_add((bytes[j] - b'0') as u64);
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
