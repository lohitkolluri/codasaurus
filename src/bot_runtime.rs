//! Runtime knobs for the GitHub bot (timeouts, comment limits, etc.).

#[derive(Debug, Clone)]
pub struct BotRuntimeConfig {
    pub review_timeout_secs: u64,
    pub max_inline_comments: usize,
    pub max_reviewer_files: usize,
    pub max_comment_bytes: usize,
    pub max_llm_diff_chars: usize,
}

impl Default for BotRuntimeConfig {
    fn default() -> Self {
        Self {
            review_timeout_secs: env_u64("CODASAURUS_REVIEW_TIMEOUT_SECS", 300),
            max_inline_comments: env_usize("CODASAURUS_MAX_INLINE_COMMENTS", 8),
            max_reviewer_files: env_usize("CODASAURUS_MAX_REVIEWER_FILES", 50),
            max_comment_bytes: env_usize("CODASAURUS_MAX_COMMENT_BYTES", 64000),
            max_llm_diff_chars: env_usize("CODASAURUS_MAX_LLM_DIFF_CHARS", 8000),
        }
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
