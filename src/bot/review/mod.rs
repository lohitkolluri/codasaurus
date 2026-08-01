//! PR review pipeline: GitHub fetch, detectors, comments, and persistence.

mod findings;
mod github;
mod llm;
mod persist;
mod pipeline;
mod reviewers;

pub use github::fetch_pull_request;
pub use pipeline::{review_pr_with_options, ReviewOptions};
