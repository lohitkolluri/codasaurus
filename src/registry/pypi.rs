use anyhow::Result;

/// Check if a PyPI package exists
pub fn check(package: &str) -> Result<Option<bool>> {
    let url = format!("https://pypi.org/pypi/{}/json", package);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;

        let resp = client.get(&url).send().await?;

        match resp.status().as_u16() {
            200 => Ok(Some(true)),
            404 => Ok(Some(false)),
            _ => Ok(None),
        }
    })
}

/// Get the latest version of a PyPI package
pub fn get_latest_version(package: &str) -> Result<Option<String>> {
    let url = format!("https://pypi.org/pypi/{}/json", package);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;

        let resp = client.get(&url).send().await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let data: serde_json::Value = resp.json().await?;
        let version = data["info"]["version"].as_str().map(|s| s.to_string());

        Ok(version)
    })
}
