use anyhow::{Context, Result};

pub async fn check_async(package: &str) -> Result<Option<bool>> {
    let url = format!("https://crates.io/api/v1/crates/{package}");
    let client = super::async_client().context("registry HTTP client not available")?;
    let resp = client
        .get(&url)
        .header(
            "User-Agent",
            concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await?;
    match resp.status().as_u16() {
        200 => Ok(Some(true)),
        404 => Ok(Some(false)),
        _ => Ok(None),
    }
}
