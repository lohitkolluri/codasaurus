//! Shared helpers used across the bot and API.

/// GitHub API related constants.
pub mod github {
    /// Number of files to fetch per API page when listing PR files.
    pub const PR_FILES_PER_PAGE: usize = 100;

    /// Maximum number of pages to fetch when listing PR files (30 * 100 = 3000 files max).
    pub const MAX_PR_FILE_PAGES: usize = 30;
}

/// Parse common truthy env / dashboard flag values (`1`, `true`, `yes`, `on`).
pub fn flag_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Read an env var as a boolean flag (missing/empty → false).
pub fn env_flag(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|v| flag_truthy(&v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_truthy_accepts_common_forms() {
        for v in ["1", "true", "TRUE", " yes ", "on", "On"] {
            assert!(flag_truthy(v), "{v}");
        }
        for v in ["", "0", "false", "no", "off", "maybe"] {
            assert!(!flag_truthy(v), "{v}");
        }
    }
}
