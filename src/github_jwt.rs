//! Shared GitHub App JWT helpers.

use anyhow::{Context, Result};
use base64::Engine;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use std::time::{SystemTime, UNIX_EPOCH};

/// Resolve the GitHub App private key from environment.
///
/// Tries `GITHUB_APP_PRIVATE_KEY` (raw PEM) first, then `GITHUB_APP_PRIVATE_KEY_B64`
/// (standard base64 — safer for PaaS `.env` files).
pub fn resolve_private_key_from_env() -> Option<String> {
    if let Ok(key) = std::env::var("GITHUB_APP_PRIVATE_KEY") {
        if !key.trim().is_empty() {
            return Some(key);
        }
    }
    let Ok(b64) = std::env::var("GITHUB_APP_PRIVATE_KEY_B64") else {
        return None;
    };
    if b64.trim().is_empty() {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

/// Like [`resolve_private_key_from_env`], but errors when neither env var is set.
pub fn require_private_key_from_env() -> Result<String> {
    resolve_private_key_from_env().ok_or_else(|| {
        anyhow::anyhow!("GITHUB_APP_PRIVATE_KEY or GITHUB_APP_PRIVATE_KEY_B64 required")
    })
}

/// Create a signed GitHub App JWT (RS256) for the given App ID and PEM private key.
pub fn create_app_jwt(app_id: &str, private_key_pem: &str) -> Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let payload = serde_json::json!({
        "iat": now.saturating_sub(60),
        "exp": now + 600,
        "iss": app_id,
    });

    let key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
        .context("Failed to parse GitHub App private key PEM")?;

    encode(&Header::new(Algorithm::RS256), &payload, &key)
        .context("Failed to create GitHub App JWT")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal RSA private key for unit tests (not used against GitHub).
    const TEST_PEM: &str = r#"-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEA0Z3VS5JJcds3xfn/ygWyF6PZX6VVwxY4H2Kz8VQwqZxqHqvF
PLACEHOLDER
-----END RSA PRIVATE KEY-----"#;

    #[test]
    fn rejects_invalid_pem() {
        let err = create_app_jwt("123", "not-a-pem").unwrap_err();
        assert!(
            err.to_string().contains("private key")
                || err.to_string().contains("PEM")
                || err.to_string().contains("key")
        );
    }

    #[test]
    fn rejects_placeholder_pem() {
        // Even with PEM-looking headers, garbage body must fail.
        assert!(create_app_jwt("123", TEST_PEM).is_err());
    }

    #[test]
    fn resolve_private_key_prefers_raw_pem() {
        std::env::remove_var("GITHUB_APP_PRIVATE_KEY");
        std::env::remove_var("GITHUB_APP_PRIVATE_KEY_B64");
        std::env::set_var(
            "GITHUB_APP_PRIVATE_KEY",
            "-----BEGIN RSA PRIVATE KEY-----\nX\n-----END RSA PRIVATE KEY-----",
        );
        let key = resolve_private_key_from_env().expect("raw pem");
        assert!(key.contains("BEGIN RSA"));
        std::env::remove_var("GITHUB_APP_PRIVATE_KEY");
    }
}
