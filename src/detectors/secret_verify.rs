//! Best-effort live-verification for a subset of secret patterns.
//!
//! Cuts false-positive noise (the #1 complaint about secret scanners): a
//! revoked/expired token is downgraded from "blocking" to "info" instead of
//! being reported with the same severity as a live credential. Verification
//! is opt-out via offline mode and fails open (`None`) on any network error
//! or unsupported pattern — we never want a flaky check to hide a real leak.

use crate::registry;
use std::time::Duration;

/// Returns `Some(true)` if the credential is live, `Some(false)` if the
/// provider rejected it, or `None` if it can't be verified (offline mode,
/// unsupported pattern, or the verification call itself failed).
pub fn verify_secret(pattern_name: &str, value: &str) -> Option<bool> {
    // Never make live network calls from unit tests — keep them hermetic.
    if cfg!(test) || registry::offline_mode() || value.len() < 8 {
        return None;
    }
    match pattern_name {
        "GitHub Token" => registry::block_on(verify_github_token(value)),
        "Slack Token" => registry::block_on(verify_slack_token(value)),
        _ => None,
    }
}

async fn verify_github_token(token: &str) -> Option<bool> {
    let client = registry::async_client().ok()?;
    let resp = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("token {token}"))
        .header("User-Agent", "codasaurus-secret-verify")
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    match resp.status().as_u16() {
        200 => Some(true),
        401 => Some(false),
        _ => None,
    }
}

async fn verify_slack_token(token: &str) -> Option<bool> {
    let client = registry::async_client().ok()?;
    let resp = client
        .post("https://slack.com/api/auth.test")
        .header("Authorization", format!("Bearer {token}"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    let json: serde_json::Value = resp.json().await.ok()?;
    json.get("ok").and_then(|v| v.as_bool())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_pattern_returns_none() {
        assert_eq!(
            verify_secret("AWS Access Key", "AKIAABCDEFGHIJKLMNOP"),
            None
        );
    }

    #[test]
    fn short_value_returns_none_without_network() {
        assert_eq!(verify_secret("GitHub Token", "short"), None);
    }
}
