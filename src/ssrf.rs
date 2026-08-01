//! SSRF guards for user-supplied URLs (custom LLM endpoints, etc.).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::Url;

/// Validate that a URL is safe to request from the server (blocks SSRF to private/metadata IPs).
///
/// When `allow_loopback` is true (Ollama local inference), `localhost` / `127.0.0.1`
/// are permitted. Private LAN and cloud-metadata addresses are always rejected.
pub fn validate_llm_base_url(raw: &str, allow_loopback: bool) -> Result<(), String> {
    let url = Url::parse(raw).map_err(|e| format!("Invalid URL: {e}"))?;

    match url.scheme() {
        "http" | "https" => {}
        other => return Err(format!("Unsupported URL scheme '{other}' — use http or https")),
    }

    let host = url
        .host_str()
        .ok_or_else(|| "URL must include a host".to_string())?;

    let host_lower = host.to_ascii_lowercase();
    let is_loopback_host = host_lower == "localhost"
        || host_lower.ends_with(".localhost")
        || host_lower == "127.0.0.1"
        || host_lower == "::1";

    if is_loopback_host {
        if allow_loopback {
            return Ok(());
        }
        return Err(format!("Host '{host}' is not allowed (SSRF protection)"));
    }

    if matches!(
        host_lower.as_str(),
        "metadata.google.internal" | "metadata" | "metadata.azure.com"
    ) || host_lower.ends_with(".internal")
        || host_lower.ends_with(".local")
    {
        return Err(format!("Host '{host}' is not allowed (SSRF protection)"));
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(ip, allow_loopback) {
            return Err(format!("IP address '{ip}' is not allowed (SSRF protection)"));
        }
    }

    Ok(())
}

fn is_blocked_ip(ip: IpAddr, allow_loopback: bool) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4, allow_loopback),
        IpAddr::V6(v6) => is_blocked_v6(v6, allow_loopback),
    }
}

fn is_blocked_v4(ip: Ipv4Addr, allow_loopback: bool) -> bool {
    if allow_loopback && ip.is_loopback() {
        return false;
    }
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.octets()[0] == 0
        // Carrier-grade NAT 100.64.0.0/10
        || (ip.octets()[0] == 100 && (ip.octets()[1] & 0xc0) == 64)
        // AWS/GCP metadata
        || ip == Ipv4Addr::new(169, 254, 169, 254)
        // Benchmarking / documentation
        || (ip.octets()[0] == 198 && (ip.octets()[1] == 18 || ip.octets()[1] == 19))
}

fn is_blocked_v6(ip: Ipv6Addr, allow_loopback: bool) -> bool {
    if allow_loopback && ip.is_loopback() {
        return false;
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip
            .to_ipv4_mapped()
            .is_some_and(|v4| is_blocked_v4(v4, allow_loopback))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_public_https() {
        assert!(validate_llm_base_url("https://openrouter.ai/api/v1", false).is_ok());
    }

    #[test]
    fn blocks_localhost_by_default() {
        assert!(validate_llm_base_url("http://localhost:11434", false).is_err());
        assert!(validate_llm_base_url("http://127.0.0.1:11434", false).is_err());
    }

    #[test]
    fn allows_localhost_when_flag_set() {
        assert!(validate_llm_base_url("http://localhost:11434", true).is_ok());
    }

    #[test]
    fn blocks_metadata() {
        assert!(validate_llm_base_url("http://169.254.169.254/latest", false).is_err());
        assert!(validate_llm_base_url("http://169.254.169.254/latest", true).is_err());
    }

    #[test]
    fn blocks_private() {
        assert!(validate_llm_base_url("http://10.0.0.1/v1", false).is_err());
        assert!(validate_llm_base_url("http://192.168.1.1/v1", true).is_err());
    }

    #[test]
    fn rejects_non_http() {
        assert!(validate_llm_base_url("ftp://example.com", false).is_err());
    }
}
