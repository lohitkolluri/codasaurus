/// Shared utility functions for the codasaurus codebase.

use std::path::Path;

/// Check if a file or directory should be hidden from analysis.
/// Skips dotfiles, common generated directories, and OS metadata files.
pub fn is_hidden(entry: &Path) -> bool {
    entry
        .file_name()
        .and_then(|n| n.to_str())
        .map(|name| {
            name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == ".svelte-kit"
                || name == "__pycache__"
                || name == "dist"
                || name == ".next"
                || name == ".git"
                || name == "venv"
                || name == ".venv"
                || name == ".env"
        })
        .unwrap_or(false)
}

/// GitHub API related constants used across the bot and action modules.
pub mod github {
    /// Number of files to fetch per API page when listing PR files.
    pub const PR_FILES_PER_PAGE: usize = 100;

    /// Maximum number of pages to fetch when listing PR files (30 * 100 = 3000 files max).
    pub const MAX_PR_FILE_PAGES: usize = 30;

    /// Maximum comment body size in bytes (GitHub's limit is ~65536).
    pub const MAX_COMMENT_BYTES: usize = 64000;

    /// Maximum number of inline comments to post in a single review.
    pub const MAX_INLINE_COMMENTS: usize = 300;

    /// Maximum number of files to suggest unique reviewers for.
    pub const MAX_REVIEWER_FILES: usize = 50;
}
