use anyhow::{Context, Result};
use std::path::Path;

/// Get the diff of staged changes
pub fn get_staged_diff() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["diff", "--cached", "--unified=3"])
        .output()
        .context("Failed to run git diff --cached. Are you in a git repository?")?;

    if !output.status.success() {
        anyhow::bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(diff)
}

/// Get diff between two refs
pub fn get_diff_between(ref_a: &str, ref_b: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["diff", ref_a, ref_b, "--unified=3"])
        .output()
        .context("Failed to run git diff between refs")?;

    if !output.status.success() {
        anyhow::bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(diff)
}

/// Get diff for a specific file or path
#[allow(dead_code)]
pub fn get_diff_for_path(path: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["diff", "--cached", "--unified=3", "--", path])
        .output()
        .context("Failed to run git diff for path")?;

    if !output.status.success() {
        anyhow::bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(diff)
}

/// Get the full content of a file from the working tree
#[allow(dead_code)]
pub fn get_file_content(path: &str) -> Result<String> {
    let full_path = Path::new(".").join(path);
    let content =
        std::fs::read_to_string(&full_path).with_context(|| format!("Failed to read {}", path))?;
    Ok(content)
}

/// Check if we're in a git repository
pub fn is_git_repo() -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the current branch name
pub fn current_branch() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context("Failed to get current branch")?;

    let branch = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();
    Ok(branch)
}

/// Get recent commit messages (up to `count`)
pub fn recent_commits(count: usize) -> Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args([
            "log",
            &format!("-{}", count),
            "--format=%B",
            "--no-color",
        ])
        .output()
        .context("Failed to get recent commits")?;

    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    let commits: Vec<String> = raw
        .split("\n\n")
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    Ok(commits)
}

/// Get the repository root path
#[allow(dead_code)]
pub fn repo_root() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to get repository root")?;

    let root = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();
    Ok(root)
}
