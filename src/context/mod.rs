//! Repo-context helpers: contribution guidelines and the rules parsed out of them.
//!
//! The local-filesystem repo scanner that used to live here is gone: the bot reads
//! repositories over the GitHub API (`bot::repo_context`), so nothing called it.

pub mod guidelines;
pub mod rules;
