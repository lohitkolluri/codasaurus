//! Whole-repo symbol graph index: tree-sitter extraction + Postgres store.

pub mod extract;
pub mod store;

use crate::config::IndexConfig;
use extract::FileIndex;

/// List every file path in the repo at `git_ref` via the Git Trees API
/// (`GET /repos/{repo}/git/trees/{sha}?recursive=1`), capped by max_files.
pub async fn list_repo_files(
    client: &reqwest::Client,
    headers: &reqwest::header::HeaderMap,
    repo_full_name: &str,
    git_ref: &str,
    max_files: usize,
) -> Result<Vec<String>, anyhow::Error> {
    let url =
        format!("https://api.github.com/repos/{repo_full_name}/git/trees/{git_ref}?recursive=1");
    let resp = crate::retry::retry_async(
        &crate::retry::RetryConfig::api_default(),
        "index_tree",
        &crate::retry::is_reqwest_error_retryable,
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
    if !resp.status().is_success() {
        anyhow::bail!("git trees API returned {}", resp.status());
    }
    let json: serde_json::Value = resp.json().await?;
    let mut paths: Vec<String> = json["tree"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|t| t["type"].as_str() == Some("blob"))
                .filter_map(|t| t["path"].as_str().map(|p| p.to_string()))
                .filter(|p| extract::language_name(p).is_some())
                .collect()
        })
        .unwrap_or_default();
    paths.truncate(max_files);
    Ok(paths)
}

/// Build the index for one repo: list files, fetch + parse the ones whose
/// language is enabled, persist. Returns the number of files indexed.
pub async fn build_repo_index(
    pool: &crate::db::DbPool,
    client: &reqwest::Client,
    headers: &reqwest::header::HeaderMap,
    repo_full_name: &str,
    git_ref: &str,
    config: &IndexConfig,
) -> Result<usize, anyhow::Error> {
    let paths = list_repo_files(client, headers, repo_full_name, git_ref, config.max_files).await?;
    let mut files: Vec<FileIndex> = Vec::new();

    for path in &paths {
        let Some(lang) = extract::language_name(path) else {
            continue;
        };
        if !config.languages.iter().any(|l| l == lang) {
            continue;
        }
        if path.starts_with("vendor/")
            || path.starts_with("node_modules/")
            || path.starts_with(".git/")
            || path.starts_with("target/")
        {
            continue;
        }
        let content = crate::bot::github_files::fetch_repo_file(
            client,
            headers,
            repo_full_name,
            path,
            git_ref,
        )
        .await;
        let Ok(Some(content)) = content else {
            continue;
        };
        if let Some(idx) = extract::extract_file(path, &content) {
            files.push(idx);
        }
    }

    store::replace_repo_index(pool, repo_full_name, &files).await?;
    Ok(files.len())
}

/// Incremental: re-index one changed file.
pub async fn reindex_file(
    pool: &crate::db::DbPool,
    client: &reqwest::Client,
    headers: &reqwest::header::HeaderMap,
    repo_full_name: &str,
    git_ref: &str,
    path: &str,
    config: &IndexConfig,
) -> Result<(), anyhow::Error> {
    let Some(lang) = extract::language_name(path) else {
        return Ok(());
    };
    if !config.languages.iter().any(|l| l == lang) {
        return Ok(());
    }
    let content =
        crate::bot::github_files::fetch_repo_file(client, headers, repo_full_name, path, git_ref)
            .await?;
    let Some(content) = content else {
        return Ok(());
    };
    if let Some(idx) = extract::extract_file(path, &content) {
        store::replace_file_index(pool, repo_full_name, &idx).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_gating_respects_config() {
        let config = IndexConfig {
            enabled: true,
            languages: vec!["python".into()],
            max_files: 1000,
        };
        assert_eq!(extract::language_name("a.py"), Some("python"));
        assert_eq!(extract::language_name("a.rs"), Some("rust"));
        assert!(config.languages.iter().any(|l| l == "python"));
        assert!(!config.languages.iter().any(|l| l == "rust"));
    }
}
