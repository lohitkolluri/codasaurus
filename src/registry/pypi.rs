use anyhow::{Context, Result};

pub async fn check_async(package: &str) -> Result<Option<bool>> {
    let url = format!("https://pypi.org/pypi/{package}/json");
    let client = super::async_client().context("registry HTTP client not available")?;
    let resp = client.get(&url).send().await?;
    match resp.status().as_u16() {
        200 => Ok(Some(true)),
        404 => Ok(Some(false)),
        _ => Ok(None),
    }
}

pub async fn metadata_async(package: &str) -> Result<Option<super::Metadata>> {
    let url = format!("https://pypi.org/pypi/{package}/json");
    let client = super::async_client().context("registry HTTP client not available")?;
    let resp = client.get(&url).send().await?;
    if resp.status().as_u16() != 200 {
        return Ok(None);
    }
    let data: serde_json::Value = resp.json().await?;
    let license = data["info"]["license"].as_str().map(str::to_string);
    Ok(Some(super::Metadata {
        name: package.to_string(),
        license,
    }))
}
