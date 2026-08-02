use crate::bot::reactions::fingerprint_from_comment_body;
use crate::learning::store::LearningStore;

pub fn fingerprint_from_thread(thread: &serde_json::Value) -> Option<String> {
    let comments = thread.get("comments")?.as_array()?;
    for comment in comments {
        if let Some(body) = comment.get("body").and_then(|b| b.as_str()) {
            if let Some(fp) = fingerprint_from_comment_body(body) {
                return Some(fp);
            }
        }
    }
    None
}

pub async fn handle_thread_event(
    pool: &crate::db::DbPool,
    action: &str,
    thread: &serde_json::Value,
    repo_full_name: &str,
    resolver_allowed: bool,
) -> anyhow::Result<bool> {
    let Some(fp) = fingerprint_from_thread(thread) else {
        tracing::debug!("review thread ignored: no fingerprint in thread comments");
        return Ok(false);
    };
    let store = LearningStore::from_pool(pool);
    match action {
        "resolved" => {
            if !resolver_allowed {
                tracing::info!(
                    repo = %repo_full_name,
                    fingerprint = %fp,
                    "resolve ignored: resolver lacks command ACL"
                );
                return Ok(false);
            }
            store
                .dismiss_fingerprint_for_repo(
                    &fp,
                    "resolve",
                    repo_full_name,
                    "dismissed via resolved review thread",
                    Some(repo_full_name),
                    None,
                    None,
                    true,
                )
                .await?;
            tracing::info!(
                repo = %repo_full_name,
                fingerprint = %fp,
                "learned dismissal from resolved thread"
            );
            Ok(true)
        }
        "unresolved" => {
            let removed = store.un_dismiss_fingerprint(&fp).await?;
            tracing::info!(
                repo = %repo_full_name,
                fingerprint = %fp,
                removed,
                "un-dismissed finding from unresolved thread"
            );
            Ok(removed)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_fp_from_thread_comment() {
        let thread = serde_json::json!({
            "node_id": "PRRT_kwDONx",
            "comments": [{
                "body": "**Secrets** · `blocking`\n\n---\n<sub>`fingerprint: abcdef012345` · `@codasaurus ignore abcdef012345`</sub>",
                "author_association": "OWNER"
            }]
        });
        assert_eq!(
            fingerprint_from_thread(&thread).as_deref(),
            Some("abcdef012345")
        );
    }

    #[test]
    fn no_fingerprint_returns_none() {
        let thread = serde_json::json!({
            "comments": [{"body": "just a question", "author_association": "MEMBER"}]
        });
        assert!(fingerprint_from_thread(&thread).is_none());
    }

    #[test]
    fn missing_comments_returns_none() {
        assert!(fingerprint_from_thread(&serde_json::json!({"node_id": "x"})).is_none());
    }
}
