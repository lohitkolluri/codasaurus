use anyhow::{Context, Result};

pub async fn metadata_async(_package: &str) -> Result<Option<super::Metadata>> {
    Ok(None)
}

/// Check whether a Go module path exists using the public proxy.
///
/// proxy.golang.org returns 200 + JSON for known modules, 404/410 for unknown,
/// and 5xx for transient issues. We treat anything except 200/404/410 as a
/// soft failure so the detector does not flag packages during a proxy outage.
pub async fn check_async(package: &str) -> Result<Option<bool>> {
    // Encode module path per Go proxy conventions: '@latest' is a literal suffix.
    let encoded = percent_encode(package);
    let url = format!("https://proxy.golang.org/{encoded}/@latest");
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
        404 | 410 => Ok(Some(false)),
        _ => Ok(None),
    }
}

/// Minimal percent-encoding for module paths in the Go proxy.
/// Go requires the module path to be escaped so that 'example.com/pkg/mod' is not
/// ambiguous with the version. The standard allows escaping only ' ' and '!' as
/// well as characters that would otherwise be separators in the request.
fn percent_encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            // Go proxy explicitly encodes these characters.
            ':' | '!' | '@' | '#' | '$' | '%' | '&' | '*' | '+' | ',' | ';' | '=' | '?' | '['
            | ']' | '(' | ')' | '"' | '\'' | ' ' | '<' | '>' | '{' | '}' | '|' | '\\' | '^'
            | '`' | '/' => {
                for b in c.encode_utf8(&mut [0; 4]).as_bytes() {
                    out.push('%');
                    out.push_str(&hex_digit(*b >> 4));
                    out.push_str(&hex_digit(*b & 0xf));
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn hex_digit(n: u8) -> String {
    std::char::from_digit(u32::from(n), 16)
        .unwrap_or('0')
        .to_uppercase()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percent_encode() {
        assert_eq!(
            percent_encode("github.com/gorilla/mux"),
            "github.com%2Fgorilla%2Fmux"
        );
        assert_eq!(
            percent_encode("github.com/example/pkg"),
            "github.com%2Fexample%2Fpkg"
        );
    }
}
