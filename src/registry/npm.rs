use anyhow::{Context, Result};

pub async fn check_async(package: &str) -> Result<Option<bool>> {
    let url = format!("https://registry.npmjs.org/{package}");
    let client = super::async_client().context("registry HTTP client not available")?;
    let resp = client
        .head(&url)
        .header("Accept", "application/vnd.npm.install-v1+json")
        .header(
            "User-Agent",
            concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await?;
    match resp.status().as_u16() {
        200 | 301 | 302 => Ok(Some(true)),
        404 | 410 => Ok(Some(false)),
        _ => Ok(None),
    }
}
