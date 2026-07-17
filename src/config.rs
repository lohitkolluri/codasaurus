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

    #[serde(default)]
    pub guidelines: GuidelinesConfig,

    #[serde(default)]
    pub tui: TuiConfig,
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

    /// Validate PR/changes against repo contribution guidelines
    #[serde(default = "default_true")]
    pub guidelines: bool,
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

/// Per-repo contribution guideline configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuidelinesConfig {
    /// Path or directory to contributing guidelines (overrides auto-discovery).
    /// Relative paths are resolved from the repo root.
    /// Also set via CONTRIBUTING_GUIDELINES env var (takes precedence).
    #[serde(default)]
    pub contributing_guidelines: Option<String>,
}

/// Interactive TUI configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TuiConfig {
    /// Editor command to open files (defaults to $EDITOR / $VISUAL)
    #[serde(default)]
    pub editor: Option<String>,
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
                guidelines: true,
            },
            behavior: BehaviorConfig {
                default_severity: "warn".to_string(),
                strict: false,
            },
            registry: RegistryConfig {
                timeout_ms: 5000,
                cache_ttl_secs: 3600,
            },
            guidelines: GuidelinesConfig::default(),
            tui: TuiConfig::default(),
        }
    }
}

/// Load config from specified path or auto-discover from parent directories.
///
/// Pass `Some("/path/to/config.toml")` to load from an explicit path, or
/// `None` to search for `.codasaurus.toml` in current/parent directories.
pub fn load(path: Option<&str>) -> Result<Config> {
    let config_path = match path {
        Some(p) => {
            let pb = PathBuf::from(p);
            if pb.exists() {
                Some(pb)
            } else {
                eprintln!(
                    "Warning: config file '{}' not found, using defaults.",
                    p
                );
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


