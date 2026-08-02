use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant};

use chrono::Utc;

use crate::db::models::*;
use crate::db::{db_execute, db_fetch_all, db_fetch_optional, DbPool};

/// Soft TTL for multi-instance deploys: a write on another replica becomes
/// visible here within this window even without shared invalidation.
/// Override with `CODASAURUS_CONFIG_CACHE_TTL_SECS` (0 disables caching).
const DEFAULT_CACHE_TTL_SECS: u64 = 60;

#[derive(Clone)]
struct CacheEntry {
    /// `None` means the key is known-absent (negative cache).
    value: Option<String>,
    loaded_at: Instant,
}

struct ConfigCache {
    entries: HashMap<String, CacheEntry>,
    /// When set, `entries` is a complete DB snapshot (plus local write-through).
    full_loaded_at: Option<Instant>,
}

impl ConfigCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            full_loaded_at: None,
        }
    }
}

static CACHE: LazyLock<RwLock<ConfigCache>> = LazyLock::new(|| RwLock::new(ConfigCache::new()));

fn cache_ttl() -> Option<Duration> {
    let secs = std::env::var("CODASAURUS_CONFIG_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_CACHE_TTL_SECS);
    if secs == 0 {
        None
    } else {
        Some(Duration::from_secs(secs))
    }
}

fn with_cache_read<R>(f: impl FnOnce(&ConfigCache) -> R) -> R {
    let guard = CACHE.read().unwrap_or_else(|e| {
        tracing::warn!("app_config cache RwLock poisoned; recovering");
        e.into_inner()
    });
    f(&guard)
}

fn with_cache_write<R>(f: impl FnOnce(&mut ConfigCache) -> R) -> R {
    let mut guard = CACHE.write().unwrap_or_else(|e| {
        tracing::warn!("app_config cache RwLock poisoned; recovering");
        e.into_inner()
    });
    f(&mut guard)
}

fn cache_get(key: &str) -> Option<Option<String>> {
    let ttl = cache_ttl()?;
    with_cache_read(|cache| {
        // Fresh full snapshot: absence is a definitive miss (no DB round-trip).
        if let Some(loaded_at) = cache.full_loaded_at {
            if loaded_at.elapsed() <= ttl {
                return Some(cache.entries.get(key).and_then(|e| e.value.clone()));
            }
        }
        let entry = cache.entries.get(key)?;
        if entry.loaded_at.elapsed() > ttl {
            return None;
        }
        Some(entry.value.clone())
    })
}

fn cache_put(key: &str, value: Option<String>) {
    if cache_ttl().is_none() {
        return;
    }
    with_cache_write(|cache| {
        cache.entries.insert(
            key.to_string(),
            CacheEntry {
                value,
                loaded_at: Instant::now(),
            },
        );
    });
}

/// Drop one key, or the whole map when `key` is `None`.
pub fn invalidate_config_cache(key: Option<&str>) {
    with_cache_write(|cache| match key {
        Some(k) => {
            cache.entries.remove(k);
            // Partial invalidation: snapshot may be incomplete.
            cache.full_loaded_at = None;
        }
        None => {
            cache.entries.clear();
            cache.full_loaded_at = None;
        }
    });
}

fn cache_put_all(entries: &[AppConfig]) {
    if cache_ttl().is_none() {
        return;
    }
    let now = Instant::now();
    with_cache_write(|cache| {
        // Full snapshot: clear so deleted keys do not linger as stale positives.
        cache.entries.clear();
        for e in entries {
            cache.entries.insert(
                e.key.clone(),
                CacheEntry {
                    value: Some(e.value.clone()),
                    loaded_at: now,
                },
            );
        }
        cache.full_loaded_at = Some(now);
    });
}

fn cache_get_all() -> Option<Vec<AppConfig>> {
    let ttl = cache_ttl()?;
    with_cache_read(|cache| {
        let loaded_at = cache.full_loaded_at?;
        if loaded_at.elapsed() > ttl {
            return None;
        }
        let now = Utc::now();
        let mut out: Vec<AppConfig> = cache
            .entries
            .iter()
            .filter_map(|(key, entry)| {
                entry.value.as_ref().map(|value| AppConfig {
                    key: key.clone(),
                    value: value.clone(),
                    updated_at: now,
                })
            })
            .collect();
        out.sort_by(|a, b| a.key.cmp(&b.key));
        Some(out)
    })
}

pub async fn get_config(pool: &DbPool, key: &str) -> Result<Option<String>, sqlx::Error> {
    if let Some(cached) = cache_get(key) {
        return Ok(cached);
    }

    let result: Option<AppConfig> = db_fetch_optional!(
        pool,
        AppConfig,
        "SELECT key, value, updated_at FROM app_config WHERE key = ?",
        key
    )?;
    let value = result.map(|c| c.value);
    cache_put(key, value.clone());
    Ok(value)
}

pub async fn set_config(pool: &DbPool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    db_execute!(
        pool,
        "INSERT INTO app_config (key, value, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
        key,
        value
    )?;
    // Write-through so this process sees the change immediately.
    cache_put(key, Some(value.to_string()));
    Ok(())
}

pub async fn delete_config(pool: &DbPool, key: &str) -> Result<(), sqlx::Error> {
    db_execute!(pool, "DELETE FROM app_config WHERE key = ?", key)?;
    cache_put(key, None);
    Ok(())
}

pub async fn get_all_config(pool: &DbPool) -> Result<Vec<AppConfig>, sqlx::Error> {
    if let Some(cached) = cache_get_all() {
        return Ok(cached);
    }
    let entries = db_fetch_all!(
        pool,
        AppConfig,
        "SELECT key, value, updated_at FROM app_config ORDER BY key"
    )?;
    cache_put_all(&entries);
    Ok(entries)
}

/// Prefetch the full map into the process cache (boot / after bulk writes).
pub async fn warm_config_cache(pool: &DbPool) {
    let _ = get_all_config(pool).await;
}

/// Dashboard keys that mirror process environment variables.
///
/// Format: `(db_key, env_key)`.
///
/// Precedence:
/// - At boot: env wins when set; otherwise DB is applied into the process env.
/// - On dashboard save: DB is updated and the process env is updated so the
///   change takes effect immediately (until the next restart, when env from
///   the host/compose again wins if set).
pub const ENV_MIRROR_KEYS: &[(&str, &str)] = &[
    ("public_url", "PUBLIC_URL"),
    ("audit_retention_days", "CODASAURUS_AUDIT_RETENTION_DAYS"),
    ("queue_workers", "CODASAURUS_QUEUE_WORKERS"),
    (
        "max_concurrent_reviews",
        "CODASAURUS_MAX_CONCURRENT_REVIEWS",
    ),
    ("hsts", "CODASAURUS_HSTS"),
    ("metrics_token", "CODASAURUS_METRICS_TOKEN"),
    ("review_timeout_secs", "CODASAURUS_REVIEW_TIMEOUT_SECS"),
    ("max_inline_comments", "CODASAURUS_MAX_INLINE_COMMENTS"),
    ("max_reviewer_files", "CODASAURUS_MAX_REVIEWER_FILES"),
    ("max_comment_bytes", "CODASAURUS_MAX_COMMENT_BYTES"),
    ("max_llm_diff_chars", "CODASAURUS_MAX_LLM_DIFF_CHARS"),
    (
        "auto_improve_max_files",
        "CODASAURUS_AUTO_IMPROVE_MAX_FILES",
    ),
    ("auto_improve_max_diff", "CODASAURUS_AUTO_IMPROVE_MAX_DIFF"),
    ("allow_local_llm", "CODASAURUS_ALLOW_LOCAL_LLM"),
    ("insecure_cookies", "CODASAURUS_INSECURE_COOKIES"),
    ("secure_cookies", "CODASAURUS_SECURE_COOKIES"),
    ("llm_daily_budget_usd", "CODASAURUS_LLM_DAILY_BUDGET_USD"),
    ("offline_mode", "CODASAURUS_OFFLINE"),
    // LLM — so `@codasaurus ask` and workers see dashboard keys without restart.
    ("llm_provider", "CODASAURUS_LLM_PROVIDER"),
    ("openrouter_api_key", "OPENROUTER_API_KEY"),
    ("llm_model", "CODASAURUS_MODEL"),
    ("llm_model_cheap", "CODASAURUS_MODEL_CHEAP"),
    ("llm_base_url", "CODASAURUS_BASE_URL"),
    ("github_app_slug", "GITHUB_APP_SLUG"),
    ("jira_base_url", "JIRA_BASE_URL"),
    ("jira_email", "JIRA_EMAIL"),
    ("jira_api_token", "JIRA_API_TOKEN"),
    ("linear_api_key", "LINEAR_API_KEY"),
    ("oidc_issuer", "OIDC_ISSUER"),
    ("oidc_client_id", "OIDC_CLIENT_ID"),
    ("oidc_client_secret", "OIDC_CLIENT_SECRET"),
    ("oidc_redirect_uri", "OIDC_REDIRECT_URI"),
    ("oidc_scopes", "OIDC_SCOPES"),
    ("oidc_allow_open_join", "OIDC_ALLOW_OPEN_JOIN"),
    ("oidc_allow_unverified_email", "OIDC_ALLOW_UNVERIFIED_EMAIL"),
    ("oidc_allow_public_client", "OIDC_ALLOW_PUBLIC_CLIENT"),
];

/// Keys that require a process restart to fully apply (concurrency / worker count).
pub const RESTART_REQUIRED_KEYS: &[&str] = &["queue_workers", "max_concurrent_reviews"];

pub fn env_key_for(db_key: &str) -> Option<&'static str> {
    ENV_MIRROR_KEYS
        .iter()
        .find(|(k, _)| *k == db_key)
        .map(|(_, env)| *env)
}

/// Copy DB values into process env when the env var is unset.
pub async fn apply_db_to_env(pool: &DbPool) {
    // One round-trip warms the cache for subsequent per-key reads.
    let Ok(entries) = get_all_config(pool).await else {
        return;
    };
    let map: HashMap<&str, &str> = entries
        .iter()
        .map(|e| (e.key.as_str(), e.value.as_str()))
        .collect();
    for (db_key, env_key) in ENV_MIRROR_KEYS {
        if std::env::var(env_key).is_ok() {
            continue;
        }
        if let Some(v) = map.get(db_key) {
            if !v.is_empty() {
                // Bridge so existing env readers pick up dashboard values.
                std::env::set_var(env_key, *v);
            }
        }
    }
}

/// Apply a dashboard setting into process env for hot reload.
pub fn apply_setting_to_env(db_key: &str, value: &str) {
    let Some(env_key) = env_key_for(db_key) else {
        return;
    };
    if value.is_empty() {
        std::env::remove_var(env_key);
    } else {
        std::env::set_var(env_key, value);
    }
}

/// Seed DB from env when the DB key is missing (ephemeral disk / compose boots).
pub async fn sync_env_mirrors_to_db(pool: &DbPool) {
    for (db_key, env_key) in ENV_MIRROR_KEYS {
        if let Ok(val) = std::env::var(env_key) {
            if val.is_empty() {
                continue;
            }
            if get_config(pool, db_key).await.ok().flatten().is_none() {
                let _ = set_config(pool, db_key, &val).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_round_trip_snapshot_and_ttl_disable() {
        invalidate_config_cache(None);
        std::env::remove_var("CODASAURUS_CONFIG_CACHE_TTL_SECS");

        cache_put("k1", Some("v1".into()));
        assert_eq!(cache_get("k1"), Some(Some("v1".into())));
        // Partial fills are not a full snapshot.
        assert!(cache_get_all().is_none());

        cache_put("missing", None);
        assert_eq!(cache_get("missing"), Some(None));

        let snap = vec![AppConfig {
            key: "alpha".into(),
            value: "1".into(),
            updated_at: Utc::now(),
        }];
        cache_put_all(&snap);
        let all = cache_get_all().expect("full snapshot");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].key, "alpha");

        // Local write-through stays coherent with the snapshot.
        cache_put("alpha", Some("2".into()));
        let all = cache_get_all().expect("snapshot after write-through");
        assert_eq!(all[0].value, "2");

        invalidate_config_cache(Some("alpha"));
        assert!(cache_get_all().is_none());

        std::env::set_var("CODASAURUS_CONFIG_CACHE_TTL_SECS", "0");
        cache_put("k2", Some("v2".into()));
        assert_eq!(cache_get("k2"), None);
        std::env::remove_var("CODASAURUS_CONFIG_CACHE_TTL_SECS");
        invalidate_config_cache(None);
    }
}
