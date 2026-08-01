/// Bound reviewer discovery to avoid exhausting an installation's API quota on a large PR.
pub(crate) const MAX_REVIEWER_FILES: usize = 8;

pub(crate) async fn suggest_reviewers(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    files: &[serde_json::Value],
    pr_author: &str,
    max_files: usize,
) -> Vec<String> {
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    let max_files = max_files.clamp(0, MAX_REVIEWER_FILES).max(0);
    if max_files == 0 || files.is_empty() {
        return Vec::new();
    }

    let author_counts = Arc::new(std::sync::Mutex::new(HashMap::<String, usize>::new()));
    let semaphore = Arc::new(Semaphore::new(5)); // keep GitHub fan-out modest

    let mut handles = Vec::with_capacity(files.len().min(max_files));
    for file in files.iter().take(max_files) {
        let filename = match file["filename"].as_str() {
            Some(f) if !f.is_empty() => f.to_string(),
            _ => continue,
        };
        let cl = client.clone();
        let auth = auth_header.to_string();
        let repo = repo_name.to_string();
        let author = pr_author.to_string();
        let counts = Arc::clone(&author_counts);
        let sem = Arc::clone(&semaphore);

        handles.push(tokio::spawn(async move {
            let _permit = match sem.acquire().await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Warning: semaphore closed: {e}");
                    return;
                }
            };

            let encoded_path = urlencoding_encode(&filename);
            let commits_url = format!(
                "https://api.github.com/repos/{repo}/commits?path={encoded_path}&per_page=3"
            );
            let commits: Vec<serde_json::Value> = match crate::retry::retry_async(
                &crate::retry::RetryConfig::quick(),
                "suggest_reviewer_commits",
                &crate::retry::is_reqwest_error_retryable,
                || async {
                    cl.get(&commits_url)
                        .header("Authorization", &auth)
                        .header("Accept", "application/vnd.github+json")
                        .header(
                            "User-Agent",
                            concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
                        )
                        .send()
                        .await
                        .map_err(Into::into)
                },
            )
            .await
            {
                Ok(resp) => match resp.error_for_status() {
                    Ok(r) => r.json::<Vec<serde_json::Value>>().await.unwrap_or_default(),
                    Err(_) => vec![],
                },
                Err(_) => vec![],
            };

            // _permit dropped here → semaphore permit returned automatically

            if !commits.is_empty() {
                let mut local = counts.lock().unwrap();
                for commit in &commits {
                    if let Some(login) = commit["author"]["login"].as_str() {
                        if login != author {
                            *local.entry(login.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }));
    }

    // Wait for all fetches to complete
    for h in handles {
        let _ = h.await;
    }

    let counts = author_counts.lock().unwrap();
    let mut reviewers: Vec<(String, usize)> = counts.clone().into_iter().collect();
    reviewers.sort_by_key(|k| std::cmp::Reverse(k.1));
    reviewers.truncate(5);
    reviewers.into_iter().map(|(name, _)| name).collect()
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
