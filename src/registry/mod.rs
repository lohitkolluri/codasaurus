use anyhow::Result;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

mod npm;
mod pypi;
mod crates_io;

/// Default cache TTL in seconds (can be overridden via `set_cache_ttl`)
static CACHE_TTL: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(3600));

/// Readers don't block each other — only writes are exclusive.
/// Avoids the double-lock TOCTOU pattern of the old Mutex cache.
static CACHE: Lazy<RwLock<HashMap<String, (bool, Instant)>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

#[allow(dead_code)]
static BLOCKING_CLIENT: Lazy<reqwest::blocking::Client> = Lazy::new(|| {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to build HTTP client")
});

/// Override the default cache TTL
#[allow(dead_code)]
pub fn set_cache_ttl(secs: u64) {
    if let Ok(mut ttl) = CACHE_TTL.lock() {
        *ttl = secs;
    }
}

/// Get the current cache TTL in seconds
#[allow(dead_code)]
pub fn get_cache_ttl() -> u64 {
    CACHE_TTL.lock().map(|t| *t).unwrap_or(3600)
}

pub fn check_package(registry: &str, package: &str) -> Result<Option<bool>> {
    let cache_key = format!("{}:{}", registry, package);
    let ttl_secs = get_cache_ttl();
    {
        let cache = CACHE.read().unwrap_or_else(|e| {
            eprintln!("Warning: cache RwLock poisoned");
            e.into_inner()
        });
        if let Some((result, time)) = cache.get(&cache_key) {
            if time.elapsed() < Duration::from_secs(ttl_secs) {
                return Ok(Some(*result));
            }
        }
    }
    let result = match registry {
        "npm" => npm::check(package),
        "pypi" => pypi::check(package),
        "crates.io" => crates_io::check(package),
        _ => return Ok(None),
    };
    if let Ok(Some(exists)) = &result {
        let mut cache = CACHE.write().unwrap_or_else(|e| {
            eprintln!("Warning: cache RwLock poisoned");
            e.into_inner()
        });
        cache.insert(cache_key, (*exists, Instant::now()));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_ttl_default() {
        assert_eq!(get_cache_ttl(), 3600);
    }

    #[test]
    fn test_cache_ttl_set_and_get() {
        set_cache_ttl(600);
        assert_eq!(get_cache_ttl(), 600);
        set_cache_ttl(3600); // restore
    }

    #[test]
    fn test_extract_package_name() {
        assert_eq!(
            crate::detectors::extract_package_name("react").as_deref(),
            Some("react")
        );
        assert_eq!(
            crate::detectors::extract_package_name("@scope/package").as_deref(),
            Some("@scope/package")
        );
        assert_eq!(
            crate::detectors::extract_package_name("lodash/fp").as_deref(),
            Some("lodash")
        );
    }
}

/// Initialize cache TTL from RegistryConfig
#[allow(dead_code)]
pub fn init_cache_from_config(config: &crate::config::Config) {
    set_cache_ttl(config.registry.cache_ttl_secs);
}

#[allow(dead_code)]
pub fn get_latest_version(registry: &str, package: &str) -> Result<Option<String>> {
    match registry {
        "npm" => npm::get_latest_version(package),
        "pypi" => pypi::get_latest_version(package),
        "crates.io" => crates_io::get_latest_version(package),
        _ => Ok(None),
    }
}

#[allow(dead_code)]
pub fn check_vulnerabilities(registry: &str, package: &str) -> Result<Vec<OsvVulnerability>> {
    let ecosystem = match registry {
        "npm" => "npm",
        "pypi" => "PyPI",
        "crates.io" => "crates.io",
        _ => return Ok(vec![]),
    };
    check_osv(ecosystem, package)
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct OsvVulnerability {
    pub id: String,
    pub summary: String,
    pub severity: String,
    pub fixed_version: Option<String>,
}

#[allow(dead_code)]
fn check_osv(ecosystem: &str, package: &str) -> Result<Vec<OsvVulnerability>> {
    let body = serde_json::json!({
        "package": {
            "name": package,
            "ecosystem": ecosystem
        }
    });
    let resp = BLOCKING_CLIENT
        .post("https://api.osv.dev/v1/query")
        .json(&body)
        .send()?;
    let data: serde_json::Value = resp.json()?;
    let vulns = data["vulns"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let id = v["id"].as_str()?.to_string();
                    let summary = v["summary"].as_str().unwrap_or("").to_string();
                    let severity = v
                        .get("database_specific")
                        .and_then(|d| d.get("severity"))
                        .and_then(|s| s.as_str())
                        .unwrap_or("UNKNOWN")
                        .to_string();
                    let fixed = v["affected"]
                        .as_array()
                        .and_then(|affected| {
                            affected.first().and_then(|a| {
                                a["ranges"]
                                    .as_array()
                                    .and_then(|ranges| {
                                        ranges.first().and_then(|r| {
                                            r["events"].as_array().and_then(|events| {
                                                events.iter().find_map(|e| {
                                                    e["fixed"].as_str().map(|s| s.to_string())
                                                })
                                            })
                                        })
                                    })
                            })
                        });
                    Some(OsvVulnerability { id, summary, severity, fixed_version: fixed })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(vulns)
}
