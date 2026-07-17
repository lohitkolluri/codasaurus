use anyhow::Result;

/// Check if an npm package exists in the registry
pub fn check(package: &str) -> Result<Option<bool>> {
    let url = format!("https://registry.npmjs.org/{}", package);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;

        let resp = client
            .head(&url)
            .header("Accept", "application/vnd.npm.install-v1+json")
            .send()
            .await?;

        match resp.status().as_u16() {
            200 | 301 | 302 => Ok(Some(true)),
            404 | 410 => Ok(Some(false)),
            _ => Ok(None),
        }
    })
}

/// Get the latest version of an npm package
pub fn get_latest_version(package: &str) -> Result<Option<String>> {
    let url = format!("https://registry.npmjs.org/{}", package);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;

        let resp = client
            .get(&url)
            .header("Accept", "application/vnd.npm.install-v1+json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let data: serde_json::Value = resp.json().await?;
        let version = data["dist-tags"]["latest"]
            .as_str()
            .map(|s| s.to_string());

        Ok(version)
    })
}
