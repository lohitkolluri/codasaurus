use crate::bot::BotConfig;
use anyhow::{Context, Result};
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn get_installation_token(config: &BotConfig) -> Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let jwt_header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    let jwt_payload = serde_json::json!({
        "iat": now.saturating_sub(60),
        "exp": now + 600,
        "iss": config.app_id,
    });

    let key = jsonwebtoken::EncodingKey::from_rsa_pem(config.private_key.as_bytes())
        .context("Failed to parse GitHub App private key")?;

    let jwt =
        jsonwebtoken::encode(&jwt_header, &jwt_payload, &key).context("Failed to create JWT")?;

    let client = reqwest::Client::new();
    let installations: Vec<serde_json::Value> = client
        .get("https://api.github.com/app/installations")
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus/0.1.0")
        .send()
        .await
        .context("Failed to get installations")?
        .json()
        .await?;

    let inst_id = installations
        .first()
        .context("No installations found — install the app on a repo first")?
        .get("id")
        .and_then(|v| v.as_i64())
        .context("Invalid installation ID")?;

    let resp: serde_json::Value = client
        .post(format!(
            "https://api.github.com/app/installations/{}/access_tokens",
            inst_id
        ))
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus/0.1.0")
        .send()
        .await
        .context("Failed to get access token")?
        .json()
        .await?;

    resp["token"]
        .as_str()
        .map(|s| s.to_string())
        .context("No token in API response")
}
