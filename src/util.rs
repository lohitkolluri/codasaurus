//! Shared constants used across the bot.

/// GitHub API related constants.
pub mod github {
    /// Number of files to fetch per API page when listing PR files.
    pub const PR_FILES_PER_PAGE: usize = 100;

    /// Maximum number of pages to fetch when listing PR files (30 * 100 = 3000 files max).
    pub const MAX_PR_FILE_PAGES: usize = 30;
}
