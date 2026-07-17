use anyhow::Result;

pub fn check(package: &str) -> Result<Option<bool>> {
    let url = format!("https://pypi.org/pypi/{}/json", package);
    let resp = super::CLIENT.get(&url).send()?;
    match resp.status().as_u16() {
        200 => Ok(Some(true)),
        404 => Ok(Some(false)),
        _ => Ok(None),
    }
}


