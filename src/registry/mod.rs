//! Package registry + OSV lookups.
//!
//! Async is the source of truth (org-scale reviews). Sync wrappers use
//! `block_on` for sync callers; the bot already runs detectors in
//! `spawn_blocking`, so sync wrappers there are safe.

use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::Semaphore;

const CACHE_MAX_SIZE: usize = 10_000;
/// Bound concurrent registry/OSV fan-out when warming caches for large PRs.
const PREFETCH_CONCURRENCY: usize = 12;
/// Short TTL for transient failures so we don't stampede npm/OSV on outages.
const SOFT_FAIL_TTL_SECS: u64 = 120;

mod crates_io;
mod npm;
mod pypi;

#[derive(Clone, Copy)]
enum PkgCacheVal {
    Exists(bool),
    /// Network/unknown failure — do not treat as "package missing".
    SoftFail,
}

static CACHE_TTL: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(3600));

static CACHE: LazyLock<RwLock<HashMap<String, (PkgCacheVal, Instant)>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// When true, registry/OSV never open sockets — cache hits only (air-gap).
static OFFLINE_MODE: AtomicBool = AtomicBool::new(false);

pub fn set_offline_mode(enabled: bool) {
    OFFLINE_MODE.store(enabled, Ordering::Relaxed);
}

pub fn offline_mode() -> bool {
    if OFFLINE_MODE.load(Ordering::Relaxed) {
        return true;
    }
    std::env::var("CODASAURUS_OFFLINE")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

type OsvCacheEntry = (Vec<OsvVulnerability>, Instant);
type OsvCacheMap = HashMap<String, OsvCacheEntry>;

static OSV_CACHE: LazyLock<RwLock<OsvCacheMap>> = LazyLock::new(|| RwLock::new(HashMap::new()));

/// Shared async HTTP client used by all registry lookups (npm, PyPI, crates.io, OSV).
static ASYNC_CLIENT: LazyLock<Option<reqwest::Client>> = LazyLock::new(|| {
    match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .connect_timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
    {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("Warning: failed to build async registry HTTP client: {e}");
            None
        }
    }
});

static FALLBACK_RT: LazyLock<Runtime> =
    LazyLock::new(|| Runtime::new().expect("failed to create registry fallback runtime"));

pub(crate) fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    match Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => FALLBACK_RT.block_on(fut),
    }
}

pub fn async_client() -> Result<&'static reqwest::Client> {
    ASYNC_CLIENT
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("async registry HTTP client not available"))
}

pub fn set_cache_ttl(secs: u64) {
    if let Ok(mut ttl) = CACHE_TTL.lock() {
        *ttl = secs;
    }
}

pub fn get_cache_ttl() -> u64 {
    CACHE_TTL.lock().map(|t| *t).unwrap_or(3600)
}

pub fn init_cache_from_config(config: &crate::config::Config) {
    set_cache_ttl(config.registry.cache_ttl_secs);
}

/// Sync API for detectors running under `spawn_blocking` (wraps async).
pub fn check_package(registry: &str, package: &str) -> Result<Option<bool>> {
    block_on(check_package_async(registry, package))
}

pub async fn check_package_async(registry: &str, package: &str) -> Result<Option<bool>> {
    let cache_key = format!("{registry}:{package}");
    let ttl_secs = get_cache_ttl();
    {
        let cache = CACHE.read().unwrap_or_else(|e| {
            eprintln!("Warning: cache RwLock poisoned");
            e.into_inner()
        });
        if let Some((result, time)) = cache.get(&cache_key) {
            let age = time.elapsed();
            match result {
                PkgCacheVal::Exists(exists) => {
                    if age < Duration::from_secs(ttl_secs) || offline_mode() {
                        crate::metrics::record_registry_cache_hit();
                        return Ok(Some(*exists));
                    }
                }
                PkgCacheVal::SoftFail => {
                    if age < Duration::from_secs(SOFT_FAIL_TTL_SECS) {
                        crate::metrics::record_registry_cache_hit();
                        return Ok(None);
                    }
                }
            }
        }
    }

    crate::metrics::record_registry_cache_miss();

    if offline_mode() {
        return Ok(None);
    }

    let result = match registry {
        "npm" => npm::check_async(package).await,
        "pypi" => pypi::check_async(package).await,
        "crates.io" => crates_io::check_async(package).await,
        _ => return Ok(None),
    };

    {
        let mut cache = CACHE.write().unwrap_or_else(|e| {
            eprintln!("Warning: cache RwLock poisoned");
            e.into_inner()
        });
        match &result {
            Ok(Some(exists)) => {
                cache.insert(cache_key, (PkgCacheVal::Exists(*exists), Instant::now()));
            }
            Ok(None) | Err(_) => {
                cache.insert(cache_key, (PkgCacheVal::SoftFail, Instant::now()));
            }
        }
        if cache.len() > CACHE_MAX_SIZE {
            evict_cache(&mut cache);
        }
    }
    result
}

pub fn check_vulnerabilities(registry: &str, package: &str) -> Result<Vec<OsvVulnerability>> {
    block_on(check_vulnerabilities_async(registry, package))
}

pub async fn check_vulnerabilities_async(
    registry: &str,
    package: &str,
) -> Result<Vec<OsvVulnerability>> {
    let ecosystem = match registry {
        "npm" => "npm",
        "pypi" => "PyPI",
        "crates.io" => "crates.io",
        _ => return Ok(vec![]),
    };
    check_osv_async(ecosystem, package).await
}

/// Concurrently warm package-existence + OSV caches for a large PR (org-scale).
pub async fn prefetch_packages(pairs: &[(String, String)]) {
    if pairs.is_empty() || offline_mode() {
        return;
    }
    let sem = std::sync::Arc::new(Semaphore::new(PREFETCH_CONCURRENCY));
    let mut handles = Vec::with_capacity(pairs.len().min(200));
    for (registry, package) in pairs.iter().take(200) {
        let registry = registry.clone();
        let package = package.clone();
        let sem = sem.clone();
        handles.push(tokio::spawn(async move {
            let Ok(_permit) = sem.acquire().await else {
                return;
            };
            let _ = check_package_async(&registry, &package).await;
            let _ = check_vulnerabilities_async(&registry, &package).await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }
}

fn evict_cache(cache: &mut HashMap<String, (PkgCacheVal, Instant)>) {
    let ttl = Duration::from_secs(get_cache_ttl());
    let now = Instant::now();
    cache.retain(|_, &mut (val, time)| match val {
        PkgCacheVal::SoftFail => now.duration_since(time) < Duration::from_secs(SOFT_FAIL_TTL_SECS),
        PkgCacheVal::Exists(_) => now.duration_since(time) < ttl,
    });

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

#[derive(Debug, Clone)]
pub struct OsvVulnerability {
    pub id: String,
    pub summary: String,
    pub severity: String,
    pub fixed_version: Option<String>,
}

async fn check_osv_async(ecosystem: &str, package: &str) -> Result<Vec<OsvVulnerability>> {
    let cache_key = format!("{ecosystem}:{package}");
    let ttl_secs = get_cache_ttl();
    {
        let cache = OSV_CACHE.read().unwrap_or_else(|e| {
            eprintln!("Warning: OSV cache RwLock poisoned");
            e.into_inner()
        });
        if let Some((vulns, time)) = cache.get(&cache_key) {
            if time.elapsed() < Duration::from_secs(ttl_secs) {
                crate::metrics::record_osv_cache_hit();
                return Ok(vulns.clone());
            }
            if offline_mode() {
                crate::metrics::record_osv_cache_hit();
                return Ok(vulns.clone());
            }
        }
    }

    crate::metrics::record_osv_cache_miss();

    if offline_mode() {
        return Ok(vec![]);
    }

    let client = async_client()?;
    let body = serde_json::json!({
        "package": {
            "name": package,
            "ecosystem": ecosystem
        }
    });

    let data: serde_json::Value = retry_async(
        &RetryConfig::api_default(),
        "osv_query",
        &is_reqwest_error_retryable,
        || async {
            client
                .post("https://api.osv.dev/v1/query")
                .json(&body)
                .send()
                .await?
                .error_for_status()?
                .json::<serde_json::Value>()
                .await
                .map_err(Into::into)
        },
    )
    .await?;

    let vulns = data["vulns"].as_array().map_or_else(Vec::new, |arr| {
        arr.iter().filter_map(extract_osv_vuln).collect()
    });

    {
        let mut cache = OSV_CACHE.write().unwrap_or_else(|e| {
            eprintln!("Warning: OSV cache RwLock poisoned");
            e.into_inner()
        });
        cache.insert(cache_key, (vulns.clone(), Instant::now()));
        if cache.len() > CACHE_MAX_SIZE {
            let ttl = Duration::from_secs(ttl_secs);
            let now = Instant::now();
            cache.retain(|_, (_, time)| now.duration_since(*time) < ttl);
            if cache.len() > CACHE_MAX_SIZE {
                let target = CACHE_MAX_SIZE / 2;
                let mut keys: Vec<(String, Instant)> =
                    cache.iter().map(|(k, (_, t))| (k.clone(), *t)).collect();
                keys.sort_unstable_by_key(|(_, t)| *t);
                for (k, _) in keys.into_iter().take(cache.len().saturating_sub(target)) {
                    cache.remove(&k);
                }
            }
        }
    }

    Ok(vulns)
}

fn extract_osv_vuln(v: &serde_json::Value) -> Option<OsvVulnerability> {
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
        .and_then(|arr| arr.first())
        .and_then(|a| a.get("ranges"))
        .and_then(|r| r.as_array())
        .and_then(|ranges| ranges.first())
        .and_then(|r| r.get("events"))
        .and_then(|e| e.as_array())
        .and_then(|events| {
            events
                .iter()
                .find_map(|e| e["fixed"].as_str().map(|s| s.to_string()))
        });

    Some(OsvVulnerability {
        id,
        summary,
        severity,
        fixed_version: fixed,
    })
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
        set_cache_ttl(3600);
    }

    #[test]
    fn test_cache_eviction_policy() {
        set_cache_ttl(3600);
        let mut cache = CACHE.write().unwrap();
        for i in 0..CACHE_MAX_SIZE + 100 {
            let key = format!("test:pkg-{i}");
            cache.insert(key, (PkgCacheVal::Exists(true), Instant::now()));
        }
        evict_cache(&mut cache);
        drop(cache);
        let cache = CACHE.read().unwrap();
        assert!(cache.len() <= CACHE_MAX_SIZE);
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

    #[tokio::test]
    async fn offline_returns_none_without_cache() {
        set_offline_mode(true);
        let r = check_package_async("npm", "codasaurus-offline-miss-xyz")
            .await
            .unwrap();
        set_offline_mode(false);
        assert!(r.is_none());
    }
}

// end of module
