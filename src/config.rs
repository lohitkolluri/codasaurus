use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{LazyLock, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub checks: CheckConfig,

    #[serde(default)]
    pub behavior: BehaviorConfig,

    #[serde(default)]
    pub registry: RegistryConfig,

    #[serde(default)]
    pub guidelines: GuidelinesConfig,

    #[serde(default)]
    pub pre_merge: PreMergeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CheckConfig {
    /// Check for imports that don't exist in the registry
    #[serde(default = "default_true")]
    pub hallucinated_imports: bool,

    /// Check for packages used but not declared in deps files
    #[serde(default = "default_true")]
    pub phantom_deps: bool,

    /// Check for known vulnerabilities via OSV.dev
    #[serde(default = "default_true")]
    pub vulnerabilities: bool,

    /// Scan for secrets and credentials
    #[serde(default = "default_true")]
    pub secrets: bool,

    /// Detect over-engineered patterns
    #[serde(default = "default_true")]
    pub over_engineering: bool,

    /// Detect boilerplate and unnecessary complexity
    #[serde(default = "default_true")]
    pub boilerplate: bool,

    /// Detect stale/outdated API usage
    #[serde(default = "default_true")]
    pub stale_api: bool,

    /// Pattern-based risky APIs (eval, XSS sinks, SQL concat, TLS skip, …)
    #[serde(default = "default_true")]
    pub risky_patterns: bool,

    /// Detect TODO/FIXME placeholders left by AI
    #[serde(default = "default_true")]
    pub todo_leaks: bool,

    /// Validate PR/changes against repo contribution guidelines
    #[serde(default = "default_true")]
    pub guidelines: bool,

    /// Graph / unused-code detector
    #[serde(default = "default_true")]
    pub graph: bool,

    /// Terraform / Kubernetes IaC red flags
    #[serde(default = "default_true")]
    pub iac: bool,

    /// Glob patterns for files/directories to skip during scanning
    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BehaviorConfig {
    /// Exit with non-zero on any finding
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegistryConfig {
    /// Cache registry responses for N seconds
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_secs: u64,
}

/// Per-repo contribution guideline configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuidelinesConfig {
    /// Path or directory to contributing guidelines (overrides auto-discovery).
    /// Relative paths are resolved from the repo root.
    /// Also set via CONTRIBUTING_GUIDELINES env var (takes precedence).
    #[serde(default)]
    pub contributing_guidelines: Option<String>,
}

/// Pre-merge check configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PreMergeConfig {
    /// Require PR description to be non-empty
    #[serde(default)]
    pub require_description: bool,
    /// Require conventional commit title format (type: description)
    #[serde(default)]
    pub require_title_convention: bool,
    /// Maximum number of blocking issues allowed (0 = none allowed)
    #[serde(default = "default_zero")]
    pub max_blocking: usize,
    /// Maximum number of warnings allowed
    #[serde(default = "default_ten")]
    pub max_warnings: usize,
}

fn default_zero() -> usize {
    0
}
fn default_ten() -> usize {
    10
}

fn default_true() -> bool {
    true
}

fn default_cache_ttl() -> u64 {
    3600
}

static DEFAULT_EXCLUDE_PATTERNS: LazyLock<Vec<String>> = LazyLock::new(|| {
    vec![
        "*.lock".into(),
        "package-lock.json".into(),
        "yarn.lock".into(),
        "Cargo.lock".into(),
        "go.sum".into(),
        "*.min.js".into(),
        "*.min.css".into(),
        "*.map".into(),
        "dist/".into(),
        "build/".into(),
        ".git/".into(),
        "node_modules/".into(),
        "target/".into(),
        ".next/".into(),
        ".nuxt/".into(),
    ]
});

fn default_exclude_patterns() -> Vec<String> {
    DEFAULT_EXCLUDE_PATTERNS.clone()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            checks: CheckConfig {
                hallucinated_imports: true,
                phantom_deps: true,
                vulnerabilities: true,
                secrets: true,
                over_engineering: true,
                boilerplate: true,
                stale_api: true,
                risky_patterns: true,
                todo_leaks: true,
                guidelines: true,
                graph: true,
                iac: true,
                exclude_patterns: default_exclude_patterns(),
            },
            behavior: BehaviorConfig { strict: false },
            registry: RegistryConfig {
                cache_ttl_secs: 3600,
            },
            guidelines: GuidelinesConfig::default(),
            pre_merge: PreMergeConfig::default(),
        }
    }
}

/// Map dashboard setting key `*_enabled` → CheckConfig field.
fn apply_enabled_flag(checks: &mut CheckConfig, key: &str, value: &str) {
    let enabled = matches!(
        value.to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on"
    );
    match key {
        "hallucinated_imports_enabled" => checks.hallucinated_imports = enabled,
        "phantom_deps_enabled" => checks.phantom_deps = enabled,
        "vulnerabilities_enabled" => checks.vulnerabilities = enabled,
        "secrets_enabled" => checks.secrets = enabled,
        "over_engineering_enabled" => checks.over_engineering = enabled,
        "boilerplate_enabled" => checks.boilerplate = enabled,
        "todo_leaks_enabled" => checks.todo_leaks = enabled,
        "stale_api_enabled" => checks.stale_api = enabled,
        "risky_patterns_enabled" => checks.risky_patterns = enabled,
        "guidelines_enabled" => checks.guidelines = enabled,
        "graph_enabled" => checks.graph = enabled,
        "iac_enabled" => checks.iac = enabled,
        _ => {}
    }
}

fn apply_behavior_flag(behavior: &mut BehaviorConfig, key: &str, value: &str) {
    if key == "default_severity" {
        // "blocking" means treat warnings as blocking (strict-ish); stored for bot policy
        let _ = value;
        let _ = behavior;
    }
}

/// Bot policy knobs loaded from DB alongside checks.
#[derive(Debug, Clone, Default)]
pub struct BotPolicy {
    /// Minimum severity to surface: blocking | warning | info
    pub min_severity: String,
}

/// Per-repo bot flags from dashboard `config_json`.
#[derive(Debug, Clone)]
pub struct RepoBotFlags {
    pub llm_enabled: bool,
    pub auto_describe: bool,
    pub auto_review_diff: bool,
    pub auto_labels: bool,
    pub update_pr_description: bool,
    pub allow_auto_fix: bool,
}

impl Default for RepoBotFlags {
    fn default() -> Self {
        Self {
            llm_enabled: true,
            auto_describe: true,
            // Opt-in: auto review_diff is the largest LLM cost; enable per-repo when wanted.
            auto_review_diff: false,
            auto_labels: true,
            update_pr_description: false,
            allow_auto_fix: false,
        }
    }
}

impl Config {
    /// Load file/env config, then overlay dashboard DB detector toggles when a pool is available.
    pub async fn load_for_bot(pool: Option<&crate::db::DbPool>) -> Self {
        let mut config = load(None).unwrap_or_default();
        if let Some(pool) = pool {
            if let Ok(entries) = crate::db::config::get_all_config(pool).await {
                for entry in entries {
                    apply_enabled_flag(&mut config.checks, &entry.key, &entry.value);
                    apply_behavior_flag(&mut config.behavior, &entry.key, &entry.value);
                    if entry.key == "exclude_patterns" && !entry.value.trim().is_empty() {
                        for part in entry.value.split([',', '\n']) {
                            let p = part.trim();
                            if !p.is_empty() {
                                config.checks.exclude_patterns.push(p.to_string());
                            }
                        }
                    }
                }
            }
        }
        config
    }

    pub async fn bot_policy(pool: Option<&crate::db::DbPool>) -> BotPolicy {
        let mut policy = BotPolicy {
            min_severity: "info".into(),
        };
        if let Some(pool) = pool {
            if let Ok(Some(v)) = crate::db::config::get_config(pool, "default_severity").await {
                if matches!(v.as_str(), "blocking" | "warning" | "info") {
                    policy.min_severity = v;
                }
            }
        }
        policy
    }

    /// Overlay per-repo `config_json` from the dashboard.
    /// Shape: `{ "detectors": {...}, "llm_enabled": bool, "auto_describe": bool,
    /// "auto_review_diff": bool, "auto_labels": bool, "update_pr_description": bool,
    /// "allow_auto_fix": bool, "exclude_patterns": ["vendor/"] }`.
    pub fn overlay_repo_config_json(&mut self, config_json: &str) -> RepoBotFlags {
        let mut flags = RepoBotFlags::default();
        let Ok(value) = serde_json::from_str::<serde_json::Value>(config_json) else {
            return flags;
        };
        if let Some(detectors) = value.get("detectors").and_then(|d| d.as_object()) {
            for (key, raw) in detectors {
                let enabled = match raw {
                    serde_json::Value::Bool(b) => *b,
                    serde_json::Value::String(s) => {
                        matches!(s.to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on")
                    }
                    _ => continue,
                };
                apply_detector_key(&mut self.checks, key, enabled);
            }
        }
        if let Some(arr) = value.get("exclude_patterns").and_then(|v| v.as_array()) {
            let extra: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .filter(|s| !s.is_empty())
                .collect();
            if !extra.is_empty() {
                self.checks.exclude_patterns.extend(extra);
            }
        }
        if let Some(v) = value.get("llm_enabled").and_then(|v| v.as_bool()) {
            flags.llm_enabled = v;
        }
        if let Some(v) = value.get("auto_describe").and_then(|v| v.as_bool()) {
            flags.auto_describe = v;
        }
        if let Some(v) = value.get("auto_review_diff").and_then(|v| v.as_bool()) {
            flags.auto_review_diff = v;
        }
        if let Some(v) = value.get("auto_labels").and_then(|v| v.as_bool()) {
            flags.auto_labels = v;
        }
        if let Some(v) = value.get("update_pr_description").and_then(|v| v.as_bool()) {
            flags.update_pr_description = v;
        }
        if let Some(v) = value.get("allow_auto_fix").and_then(|v| v.as_bool()) {
            flags.allow_auto_fix = v;
        }
        flags
    }
}

fn apply_detector_key(checks: &mut CheckConfig, key: &str, enabled: bool) {
    match key {
        "hallucinated_imports" => checks.hallucinated_imports = enabled,
        "phantom_deps" => checks.phantom_deps = enabled,
        "vulnerabilities" => checks.vulnerabilities = enabled,
        "secrets" => checks.secrets = enabled,
        "over_engineering" => checks.over_engineering = enabled,
        "boilerplate" => checks.boilerplate = enabled,
        "todo_leaks" => checks.todo_leaks = enabled,
        "stale_api" => checks.stale_api = enabled,
        "risky_patterns" => checks.risky_patterns = enabled,
        "guidelines" => checks.guidelines = enabled,
        "graph" => checks.graph = enabled,
        "iac" => checks.iac = enabled,
        _ => {}
    }
}

/// Load config from specified path or auto-discover from parent directories.
///
/// Pass `Some("/path/to/config.toml")` to load from an explicit path, or
/// `None` to search for `.codasaurus.toml` in current/parent directories.
pub fn load(path: Option<&str>) -> Result<Config> {
    // Support CODASAURUS_CONFIG env var as override when no explicit path given
    let env_path = std::env::var("CODASAURUS_CONFIG").ok();
    let resolved = path.or(env_path.as_deref()).filter(|s| !s.is_empty());
    let config_path = match resolved {
        Some(p) => {
            let pb = PathBuf::from(p);
            if pb.exists() {
                Some(pb)
            } else {
                eprintln!("Warning: config file '{p}' not found, using defaults.");
                None
            }
        }
        None => find_config()?,
    };
    let config = match config_path {
        Some(path) => {
            let contents = std::fs::read_to_string(&path)?;
            let config: Config = toml::from_str(&contents)?;
            config
        }
        None => Config::default(),
    };

    crate::registry::init_cache_from_config(&config);

    Ok(config)
}

static CONFIG_PATH_CACHE: OnceLock<std::sync::Mutex<Option<PathBuf>>> = OnceLock::new();

fn find_config() -> Result<Option<PathBuf>> {
    let cache = CONFIG_PATH_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some(ref path) = *guard {
            if path.exists() {
                return Ok(Some(path.clone()));
            }
        }
    }

    let cwd = std::env::current_dir()?;
    let mut current = Some(cwd.as_path());
    let resolved = loop {
        match current {
            Some(dir) => {
                let candidate = dir.join(".codasaurus.toml");
                if candidate.exists() {
                    break Some(candidate);
                }
                current = dir.parent();
            }
            None => break None,
        }
    };

    if let Ok(mut guard) = cache.lock() {
        *guard = resolved.clone();
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_repo_detectors_and_llm_flag() {
        let mut cfg = Config::default();
        assert!(cfg.checks.secrets);
        let flags = cfg.overlay_repo_config_json(
            r#"{"detectors":{"secrets":false,"graph":false},"llm_enabled":false,"auto_describe":false}"#,
        );
        assert!(!flags.llm_enabled);
        assert!(!flags.auto_describe);
        assert!(!flags.auto_review_diff);
        assert!(!cfg.checks.secrets);
        assert!(!cfg.checks.graph);
        assert!(cfg.checks.hallucinated_imports);
    }
}
