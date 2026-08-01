//! Fetch repository files via the GitHub Contents API (no local clone).

use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use anyhow::Result;
use base64::Engine;
use reqwest::header::HeaderMap;

/// Soft cap so we never pull huge binaries / lockfiles into memory.
const MAX_FILE_BYTES: usize = 512_000;

/// GET `/repos/{repo}/contents/{path}?ref=` and decode file content.
/// Returns `Ok(None)` on 404 or unsupported types (directory / symlink).
pub async fn fetch_repo_file(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    path: &str,
    git_ref: &str,
) -> Result<Option<String>> {
    if path.is_empty() || git_ref.is_empty() {
        return Ok(None);
    }
    let encoded = path
        .split('/')
        .map(|seg| {
            let mut out = String::new();
            for b in seg.bytes() {
                match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        out.push(b as char);
                    }
                    _ => out.push_str(&format!("%{b:02X}")),
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("/");

    let url = format!(
        "https://api.github.com/repos/{repo}/contents/{encoded}?ref={}",
        urlencoding_query(git_ref)
    );

    let resp = retry_async(
        &RetryConfig::api_default(),
        "fetch_repo_file",
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
    .await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        tracing::debug!(status = %resp.status(), path, "contents API non-success");
        return Ok(None);
    }

    let body: serde_json::Value = resp.json().await?;
    if body.get("type").and_then(|t| t.as_str()) != Some("file") {
        return Ok(None);
    }
    let size = body["size"].as_u64().unwrap_or(0) as usize;
    if size > MAX_FILE_BYTES {
        tracing::debug!(path, size, "skipping oversized repo file");
        return Ok(None);
    }
    let encoded = match body["content"].as_str() {
        Some(c) => c,
        None => return Ok(None),
    };
    // GitHub returns base64 with newlines
    let cleaned: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .map_err(|e| anyhow::anyhow!("base64 decode failed for {path}: {e}"))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Fetch file content and blob SHA (needed for Contents API updates).
pub async fn fetch_repo_file_with_sha(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    path: &str,
    git_ref: &str,
) -> Result<Option<(String, String)>> {
    if path.is_empty() || git_ref.is_empty() {
        return Ok(None);
    }
    let encoded = path
        .split('/')
        .map(|seg| {
            let mut out = String::new();
            for b in seg.bytes() {
                match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        out.push(b as char);
                    }
                    _ => out.push_str(&format!("%{b:02X}")),
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("/");

    let url = format!(
        "https://api.github.com/repos/{repo}/contents/{encoded}?ref={}",
        urlencoding_query(git_ref)
    );
    let resp = retry_async(
        &RetryConfig::api_default(),
        "fetch_repo_file_sha",
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
    .await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND || !resp.status().is_success() {
        return Ok(None);
    }
    let body: serde_json::Value = resp.json().await?;
    if body.get("type").and_then(|t| t.as_str()) != Some("file") {
        return Ok(None);
    }
    let sha = body["sha"].as_str().unwrap_or("").to_string();
    if sha.is_empty() {
        return Ok(None);
    }
    let size = body["size"].as_u64().unwrap_or(0) as usize;
    if size > MAX_FILE_BYTES {
        return Ok(None);
    }
    let encoded = match body["content"].as_str() {
        Some(c) => c,
        None => return Ok(None),
    };
    let cleaned: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .map_err(|e| anyhow::anyhow!("base64 decode failed for {path}: {e}"))?;
    Ok(Some((String::from_utf8_lossy(&bytes).into_owned(), sha)))
}

/// Create or update a file on a branch via Contents API.
#[allow(clippy::too_many_arguments)]
pub async fn put_repo_file(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    path: &str,
    branch: &str,
    content: &str,
    sha: &str,
    message: &str,
) -> Result<()> {
    let encoded = path
        .split('/')
        .map(|seg| {
            let mut out = String::new();
            for b in seg.bytes() {
                match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        out.push(b as char);
                    }
                    _ => out.push_str(&format!("%{b:02X}")),
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("/");
    let url = format!("https://api.github.com/repos/{repo}/contents/{encoded}");
    let b64 = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
    let body = serde_json::json!({
        "message": message,
        "content": b64,
        "branch": branch,
        "sha": sha,
    });
    let resp = retry_async(
        &RetryConfig::api_default(),
        "put_repo_file",
        &is_reqwest_error_retryable,
        || async {
            client
                .put(&url)
                .headers(headers.clone())
                .json(&body)
                .send()
                .await
                .map_err(Into::into)
        },
    )
    .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("put_repo_file failed: {status} {text}");
    }
    Ok(())
}

/// Try paths in order; return the first that exists.
pub async fn fetch_first_existing(
    client: &reqwest::Client,
    headers: &HeaderMap,
    repo: &str,
    paths: &[&str],
    git_ref: &str,
) -> Result<Option<(String, String)>> {
    for path in paths {
        if let Some(content) = fetch_repo_file(client, headers, repo, path, git_ref).await? {
            return Ok(Some(((*path).to_string(), content)));
        }
    }
    Ok(None)
}

fn urlencoding_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_encoding_keeps_simple_refs() {
        assert_eq!(urlencoding_query("main"), "main");
        assert!(urlencoding_query("feat/foo").contains('%'));
    }
}
