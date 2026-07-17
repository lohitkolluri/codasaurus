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
    let url = format!("https://crates.io/api/v1/crates/{}", package);
    let resp = CLIENT
        .get(&url)
        .header("User-Agent", "codasaurus/0.1.0")
        .send()?;
    match resp.status().as_u16() {
        200 => Ok(Some(true)),
        404 => Ok(Some(false)),
        _ => Ok(None),
    }
}

pub fn get_latest_version(package: &str) -> Result<Option<String>> {
    let url = format!("https://crates.io/api/v1/crates/{}", package);
    let resp = CLIENT
        .get(&url)
        .header("User-Agent", "codasaurus/0.1.0")
        .send()?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let data: serde_json::Value = resp.json()?;
    Ok(data["crate"]["max_version"].as_str().map(|s| s.to_string()))
}
