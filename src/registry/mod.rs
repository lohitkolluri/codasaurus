use anyhow::Result;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

/// Maximum number of cache entries before eviction of expired/stale entries.
const CACHE_MAX_SIZE: usize = 10_000;

mod crates_io;
mod npm;
mod pypi;

/// Default cache TTL in seconds (can be overridden via `set_cache_ttl`)
static CACHE_TTL: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(3600));

/// Readers don't block each other — only writes are exclusive.
/// Avoids the double-lock TOCTOU pattern of the old Mutex cache.
static CACHE: LazyLock<RwLock<HashMap<String, (bool, Instant)>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Shared HTTP client used by all registry lookups (npm, PyPI, crates.io, OSV).
static CLIENT: LazyLock<Option<reqwest::blocking::Client>> = LazyLock::new(|| {
    match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(5)
        .build()
    {
        Ok(client) => Some(client),
        Err(e) => {
            eprintln!("Warning: failed to build registry HTTP client: {}", e);
            None
        }
    }
});

pub fn set_cache_ttl(secs: u64) {
    if let Ok(mut ttl) = CACHE_TTL.lock() {
        *ttl = secs;
    }
}

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

        if cache.len() > CACHE_MAX_SIZE {
            evict_cache(&mut cache);
        }
    }
    result
}

/// Evict expired entries, then trim to half capacity in O(n log n).
///
/// The previous implementation repeatedly searched the whole map for its oldest
/// key, making a full trim quadratic and very expensive under cache pressure.
fn evict_cache(cache: &mut HashMap<String, (bool, Instant)>) {
    let ttl = Duration::from_secs(get_cache_ttl());
    let now = Instant::now();
    cache.retain(|_, &mut (_, time)| now.duration_since(time) < ttl);

    let target_len = CACHE_MAX_SIZE / 2;
    if cache.len() <= target_len {
        return;
    }

    let mut entries: Vec<(String, Instant)> = cache
        .iter()
        .map(|(key, &(_, timestamp))| (key.clone(), timestamp))
        .collect();
    entries.sort_unstable_by_key(|(_, timestamp)| *timestamp);
    for (key, _) in entries.into_iter().take(cache.len() - target_len) {
        cache.remove(&key);
    }
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
    fn test_cache_eviction_policy() {
        set_cache_ttl(3600);
        let mut cache = CACHE.write().unwrap();
        // Fill cache beyond max size with varied timestamps so oldest-pick is deterministic
        for i in 0..CACHE_MAX_SIZE + 100 {
            let key = format!("test:pkg-{}", i);
            cache.insert(key, (true, Instant::now()));
        }

        evict_cache(&mut cache);
        drop(cache);

        // Verify cache is bounded
        let cache = CACHE.read().unwrap();
        assert!(cache.len() <= CACHE_MAX_SIZE, "cache should be bounded");
        assert!(cache.len() >= CACHE_MAX_SIZE / 2);
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

pub fn init_cache_from_config(config: &crate::config::Config) {
    set_cache_ttl(config.registry.cache_ttl_secs);
}

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

fn check_osv(ecosystem: &str, package: &str) -> Result<Vec<OsvVulnerability>> {
    let client = CLIENT.as_ref().ok_or_else(|| {
        anyhow::anyhow!("registry HTTP client not available (failed to initialize)")
    })?;
    let body = serde_json::json!({
        "package": {
            "name": package,
            "ecosystem": ecosystem
        }
    });
    let resp = client
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
                    let fixed = v["affected"].as_array().and_then(|affected| {
                        affected.first().and_then(|a| {
                            a["ranges"].as_array().and_then(|ranges| {
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
}
