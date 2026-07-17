use anyhow::Result;

pub fn check(package: &str) -> Result<Option<bool>> {
    let url = format!("https://registry.npmjs.org/{}", package);
    let resp = super::CLIENT
        .head(&url)
        .header("Accept", "application/vnd.npm.install-v1+json")
        .send()?;
    match resp.status().as_u16() {
        200 | 301 | 302 => Ok(Some(true)),
        404 | 410 => Ok(Some(false)),
        _ => Ok(None),
    }
}


