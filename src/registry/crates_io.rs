use anyhow::Result;

pub fn check(package: &str) -> Result<Option<bool>> {
    let url = format!("https://crates.io/api/v1/crates/{}", package);
    let resp = super::CLIENT
        .get(&url)
        .header("User-Agent", "codasaurus/0.1.0")
        .send()?;
    match resp.status().as_u16() {
        200 => Ok(Some(true)),
        404 => Ok(Some(false)),
        _ => Ok(None),
    }
}


