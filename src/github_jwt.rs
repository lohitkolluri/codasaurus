//! Shared GitHub App JWT helpers.

use anyhow::{Context, Result};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use std::time::{SystemTime, UNIX_EPOCH};

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

    encode(&Header::new(Algorithm::RS256), &payload, &key).context("Failed to create GitHub App JWT")
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
        assert!(err.to_string().contains("private key") || err.to_string().contains("PEM") || err.to_string().contains("key"));
    }

    #[test]
    fn rejects_placeholder_pem() {
        // Even with PEM-looking headers, garbage body must fail.
        assert!(create_app_jwt("123", TEST_PEM).is_err());
    }
}
