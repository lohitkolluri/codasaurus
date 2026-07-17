use anyhow::Result;
use once_cell::sync::Lazy;
use std::time::Duration;

static CLIENT: Lazy<reqwest::blocking::Client> = Lazy::new(|| {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .expect("Failed to create HTTP client")
});

pub fn check(package: &str) -> Result<Option<bool>> {
    let url = format!("https://registry.npmjs.org/{}", package);
    let resp = CLIENT
        .head(&url)
        .header("Accept", "application/vnd.npm.install-v1+json")
        .send()?;
    match resp.status().as_u16() {
        200 | 301 | 302 => Ok(Some(true)),
        404 | 410 => Ok(Some(false)),
        _ => Ok(None),
    }
}

#[allow(dead_code)]
pub fn get_latest_version(package: &str) -> Result<Option<String>> {
    let url = format!("https://registry.npmjs.org/{}", package);
    let resp = CLIENT
        .get(&url)
        .header("Accept", "application/vnd.npm.install-v1+json")
        .send()?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let data: serde_json::Value = resp.json()?;
    Ok(data["dist-tags"]["latest"].as_str().map(|s| s.to_string()))
}
