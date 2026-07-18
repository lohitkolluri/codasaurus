use crate::bot::BotConfig;
use anyhow::{Context, Result};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Build a production-configured HTTP client with timeouts and connection pooling.
fn build_github_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(5)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_nodelay(true)
        .build()?)
}

pub async fn get_installation_token(config: &BotConfig, installation_id: Option<i64>) -> Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
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

    let client = build_github_client().context("Failed to build GitHub API client")?;

    let inst_id = if let Some(iid) = installation_id {
        iid
    } else {
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
        installations
            .first()
            .context("No installations found — install the app on a repo first")?
            .get("id")
            .and_then(|v| v.as_i64())
            .context("Invalid installation ID")?
    };

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
