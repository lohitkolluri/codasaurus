use anyhow::{Context, Result};

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

    let diff = String::from_utf8(output.stdout)?;
    Ok(diff)
}

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

    let diff = String::from_utf8(output.stdout)?;
    Ok(diff)
}



pub fn is_git_repo() -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn current_branch() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context("Failed to get current branch")?;

    let branch = String::from_utf8(output.stdout)?.trim().to_string();
    if branch == "HEAD" {
        Ok("detached".to_string())
    } else {
        Ok(branch)
    }
}

/// Get recent commit messages (up to `count`)
pub fn recent_commits(count: usize) -> Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["log", &format!("-{}", count), "--format=%B", "--no-color"])
        .output()
        .context("Failed to get recent commits")?;

    let raw = String::from_utf8(output.stdout)?;
    let commits: Vec<String> = raw
        .split("\n\n")
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    Ok(commits)
}

pub fn repo_root() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to get repository root")?;

    let root = String::from_utf8(output.stdout)?.trim().to_string();
    Ok(root)
}
