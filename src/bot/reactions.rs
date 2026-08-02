//! Learn from GitHub reactions on Codasaurus finding comments.

use crate::learning::store::LearningStore;

/// Reaction contents that mean "false positive / dismiss".
const DISMISS_REACTIONS: &[&str] = &["-1", "confused"];
/// Soft positive signal (telemetry / optional always_warn reverse — we only log).
const LGTM_REACTIONS: &[&str] = &["+1", "heart", "hooray", "rocket"];

/// Extract a finding fingerprint from a Codasaurus comment body.
pub fn fingerprint_from_comment_body(body: &str) -> Option<String> {
    // Prefer explicit fingerprint: line
    for line in body.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("`fingerprint:") {
            let fp = rest
                .trim()
                .trim_start_matches('`')
                .split(|c: char| c == '`' || c.is_whitespace() || c == '·')
                .next()
                .unwrap_or("")
                .trim();
            if fp.len() >= 8 {
                return Some(fp.to_string());
            }
        }
        if let Some(idx) = t.find("fingerprint:") {
            let rest = &t[idx + "fingerprint:".len()..];
            let fp = rest
                .trim()
                .trim_matches('`')
                .split(|c: char| c == '`' || c.is_whitespace() || c == '·')
                .next()
                .unwrap_or("")
                .trim();
            if fp.len() >= 8 {
                return Some(fp.to_string());
            }
        }
    }
    // Fallback: `@codasaurus ignore <fp>` in the footer
    for prefix in ["@codasaurus ignore ", "@codasaurus-bot ignore "] {
        if let Some(idx) = body.to_ascii_lowercase().find(&prefix.to_ascii_lowercase()) {
            let rest = &body[idx + prefix.len()..];
            let fp = rest.split_whitespace().next().unwrap_or("").trim();
            if fp.len() >= 8 {
                return Some(fp.to_string());
            }
        }
    }
    None
}

pub fn is_dismiss_reaction(content: &str) -> bool {
    DISMISS_REACTIONS.contains(&content)
}

pub fn is_lgtm_reaction(content: &str) -> bool {
    LGTM_REACTIONS.contains(&content)
}

/// Handle a `reaction` webhook: 👎 / confused → dismiss fingerprint from comment body.
pub async fn handle_reaction_event(
    pool: &crate::db::DbPool,
    action: &str,
    reaction_content: &str,
    comment_body: &str,
    repo_full_name: &str,
) -> anyhow::Result<bool> {
    if action != "created" {
        return Ok(false);
    }
    let Some(fp) = fingerprint_from_comment_body(comment_body) else {
        tracing::debug!("reaction ignored: no fingerprint in comment");
        return Ok(false);
    };

    if is_dismiss_reaction(reaction_content) {
        let store = LearningStore::from_pool(pool);
        store
            .dismiss_fingerprint_for_repo(
                &fp,
                "reaction",
                repo_full_name,
                &format!("dismissed via {reaction_content} reaction"),
                Some(repo_full_name),
                None,
                None,
                false,
            )
            .await?;
        tracing::info!(
            repo = %repo_full_name,
            fingerprint = %fp,
            reaction = %reaction_content,
            "learned dismissal from reaction"
        );
        return Ok(true);
    }

    if is_lgtm_reaction(reaction_content) {
        tracing::debug!(
            repo = %repo_full_name,
            fingerprint = %fp,
            reaction = %reaction_content,
            "positive reaction on finding (no dismiss)"
        );
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_fp_from_footer() {
        let body = "**Secrets** · `blocking`\n\n---\n<sub>`fingerprint: abcdef012345` · `@codasaurus ignore abcdef012345`</sub>";
        assert_eq!(
            fingerprint_from_comment_body(body).as_deref(),
            Some("abcdef012345")
        );
    }

    #[test]
    fn dismiss_reactions() {
        assert!(is_dismiss_reaction("-1"));
        assert!(is_dismiss_reaction("confused"));
        assert!(!is_dismiss_reaction("+1"));
    }
}
