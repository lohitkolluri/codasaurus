use anyhow::Result;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

mod npm;
mod pypi;
mod crates_io;

/// Cache for registry responses
static CACHE: once_cell::sync::Lazy<Mutex<HashMap<String, (bool, Instant)>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));

/// Check if a package exists in the given registry
pub fn check_package(registry: &str, package: &str) -> Result<Option<bool>> {
    let cache_key = format!("{}:{}", registry, package);

    // Check cache
    {
        let cache = CACHE.lock().unwrap();
        if let Some((result, time)) = cache.get(&cache_key) {
            if time.elapsed() < Duration::from_secs(3600) {
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

    // Update cache
    if let Ok(Some(exists)) = &result {
        let mut cache = CACHE.lock().unwrap();
        cache.insert(cache_key, (*exists, Instant::now()));
    }

    result
}

/// Get the latest version of a package
pub fn get_latest_version(registry: &str, package: &str) -> Result<Option<String>> {
    match registry {
        "npm" => npm::get_latest_version(package),
        "pypi" => pypi::get_latest_version(package),
        "crates.io" => crates_io::get_latest_version(package),
        _ => Ok(None),
    }
}

/// Check a package against OSV.dev vulnerability database
pub fn check_vulnerabilities(registry: &str, package: &str) -> Result<Vec<OsvVulnerability>> {
    let ecosystem = match registry {
        "npm" => "npm",
        "pypi" => "PyPI",
        "crates.io" => "crates.io",
        _ => return Ok(vec![]),
    };

    check_osv(ecosystem, package)
}

#[derive(Debug, Clone)]
pub struct OsvVulnerability {
    pub id: String,
    pub summary: String,
    pub severity: String,
    pub fixed_version: Option<String>,
}

/// Query OSV.dev API for known vulnerabilities
fn check_osv(ecosystem: &str, package: &str) -> Result<Vec<OsvVulnerability>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        let body = serde_json::json!({
            "package": {
                "name": package,
                "ecosystem": ecosystem
            }
        });

        let resp = client
            .post("https://api.osv.dev/v1/query")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(vec![]);
        }

        let data: serde_json::Value = resp.json().await?;
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

                        // Extract fixed version from affected ranges
                        let fixed = v["affected"]
                            .as_array()
                            .and_then(|affected| {
                                affected.first().and_then(|a| {
                                    a["ranges"]
                                        .as_array()
                                        .and_then(|ranges| {
                                            ranges.first().and_then(|r| {
                                                r["events"].as_array().and_then(|events| {
                                                    events
                                                        .iter()
                                                        .find_map(|e| {
                                                            e["fixed"].as_str().map(|s| s.to_string())
                                                        })
                                                })
                                            })
                                        })
                                })
                            });

                        Some(OsvVulnerability {
                            id,
                            summary,
                            severity,
                            fixed_version: fixed,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(vulns)
    })
}
