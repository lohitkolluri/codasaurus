use anyhow::{Context, Result};

pub fn check(package: &str) -> Result<Option<bool>> {
    let url = format!("https://pypi.org/pypi/{}/json", package);
    let client = super::CLIENT.as_ref()
        .context("registry HTTP client not available (failed to initialize)")?;
    let resp = client.get(&url).send()?;
    match resp.status().as_u16() {
        200 => Ok(Some(true)),
        404 => Ok(Some(false)),
        _ => Ok(None),
    }
}


