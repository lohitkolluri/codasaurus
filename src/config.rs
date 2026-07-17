use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub checks: CheckConfig,

    #[serde(default)]
    pub behavior: BehaviorConfig,

    #[serde(default)]
    pub registry: RegistryConfig,
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
    #[serde(default = "default_false")]
    pub stale_api: bool,

    /// Detect TODO/FIXME placeholders left by AI
    #[serde(default = "default_true")]
    pub todo_leaks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BehaviorConfig {
    /// Severity level: "block", "warn", "info"
    #[serde(default = "default_severity")]
    pub default_severity: String,

    /// Exit with non-zero on any finding
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegistryConfig {
    /// Timeout for registry API calls in milliseconds
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,

    /// Cache registry responses for N seconds
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_secs: u64,
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_severity() -> String {
    "warn".to_string()
}

fn default_timeout() -> u64 {
    5000
}

fn default_cache_ttl() -> u64 {
    3600
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
                stale_api: false,
                todo_leaks: true,
            },
            behavior: BehaviorConfig {
                default_severity: "warn".to_string(),
                strict: false,
            },
            registry: RegistryConfig {
                timeout_ms: 5000,
                cache_ttl_secs: 3600,
            },
        }
    }
}

/// Load config from .codasaurus.toml in current or parent directories
pub fn load() -> Result<Config> {
    let config_path = find_config()?;
    match config_path {
        Some(path) => {
            let contents = std::fs::read_to_string(&path)?;
            let config: Config = toml::from_str(&contents)?;
            Ok(config)
        }
        None => Ok(Config::default()),
    }
}

fn find_config() -> Result<Option<PathBuf>> {
    let cwd = std::env::current_dir()?;
    let mut current = Some(cwd.as_path());

    while let Some(dir) = current {
        let candidate = dir.join(".codasaurus.toml");
        if candidate.exists() {
            return Ok(Some(candidate));
        }
        current = dir.parent();
    }

    Ok(None)
}

/// The default config file content for `codasaurus init`
#[allow(dead_code)]
pub fn default_config_content() -> &'static str {
    r#"# Codasaurus configuration
# See https://github.com/lohitkolluri/codasaurus for docs

[checks]
hallucinated_imports = true
phantom_deps = true
vulnerabilities = true
secrets = true
over_engineering = true
boilerplate = true
stale_api = false
todo_leaks = true

[behavior]
default_severity = "warn"
strict = false

[registry]
timeout_ms = 5000
cache_ttl_secs = 3600
"#
}
