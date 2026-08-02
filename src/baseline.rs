use crate::db::DbPool;
use crate::detectors::Finding;
use anyhow::Result;
use std::collections::HashSet;

/// Parse a GitHub diff `patch` and return the set of new-file line numbers that
/// were added or modified (i.e., "new code" lines).
///
/// GitHub hunks look like `@@ -a,b +c,d @@` where lines starting with `+` are
/// new lines, `-` are removed, and ` ` are context. Removed lines do not have
/// a line number in the new file.
pub fn parse_patch_changed_lines(patch: &str) -> HashSet<usize> {
    let mut lines = HashSet::new();
    let mut new_line: usize = 0;
    for line in patch.lines() {
        if let Some(header) = line.strip_prefix("@@") {
            if let Some(rest) = header.split_once(" +") {
                if let Some(rhs) = rest.1.split(" @@").next() {
                    let (start, _count) = parse_hunk_start_count(rhs);
                    new_line = start;
                }
            }
        } else if line.starts_with('+') {
            lines.insert(new_line);
            new_line += 1;
        } else if line.starts_with('-') {
            // Removed lines have no line number in the new file.
        } else if line.starts_with(' ') {
            new_line += 1;
        } else if line == "\\ No newline at end of file" {
            // no-op
        }
    }
    lines
}

fn parse_hunk_start_count(s: &str) -> (usize, usize) {
    let s = s.trim();
    if let Some((start_s, count_s)) = s.split_once(',') {
        let start = start_s.parse::<usize>().unwrap_or(1);
        let count = count_s.parse::<usize>().unwrap_or(1);
        (start, count)
    } else {
        let start = s.parse::<usize>().unwrap_or(1);
        (start, 1)
    }
}

/// Persist the new-code line numbers for a PR so that the baseline filter can
/// query them during review.
pub async fn save_pr_diff_lines(
    pool: &DbPool,
    repo_full_name: &str,
    pr_number: i64,
    file_path: &str,
    changed_lines: &HashSet<usize>,
) -> Result<()> {
    let repo = repo_full_name;
    let pr = pr_number;
    let file = file_path;
    for line in changed_lines {
        let l = *line as i32;
        sqlx::query(
            "INSERT INTO pr_diff_lines (repo_full_name, pr_number, file_path, line)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (repo_full_name, pr_number, file_path, line) DO NOTHING",
        )
        .bind(repo)
        .bind(pr)
        .bind(file)
        .bind(l)
        .execute(pool.as_pg())
        .await?;
    }
    Ok(())
}

/// Remove any previously stored diff lines for this PR so that re-reviews are
/// idempotent.
pub async fn clear_pr_diff_lines(
    pool: &DbPool,
    repo_full_name: &str,
    pr_number: i64,
) -> Result<()> {
    sqlx::query("DELETE FROM pr_diff_lines WHERE repo_full_name = $1 AND pr_number = $2")
        .bind(repo_full_name)
        .bind(pr_number)
        .execute(pool.as_pg())
        .await?;
    Ok(())
}

/// Insert a finding into the baseline so that future PRs will not re-flag it.
async fn record_baseline(pool: &DbPool, repo_full_name: &str, fingerprint: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO finding_baseline (repo_full_name, fingerprint, first_seen_at, last_seen_at)
         VALUES ($1, $2, NOW(), NOW())
         ON CONFLICT (repo_full_name, fingerprint)
         DO UPDATE SET last_seen_at = NOW()",
    )
    .bind(repo_full_name)
    .bind(fingerprint)
    .execute(pool.as_pg())
    .await?;
    Ok(())
}

/// Return true if a finding has already been recorded as pre-existing.
async fn is_baseline(pool: &DbPool, repo_full_name: &str, fingerprint: &str) -> Result<bool> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM finding_baseline
         WHERE repo_full_name = $1 AND fingerprint = $2",
    )
    .bind(repo_full_name)
    .bind(fingerprint)
    .fetch_one(pool.as_pg())
    .await?;
    Ok(count > 0)
}

/// True if the given file path and line is a new-code line in the current PR.
async fn is_new_code_line(
    pool: &DbPool,
    repo_full_name: &str,
    pr_number: i64,
    file_path: &str,
    line: usize,
) -> Result<bool> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pr_diff_lines
         WHERE repo_full_name = $1 AND pr_number = $2 AND file_path = $3 AND line = $4",
    )
    .bind(repo_full_name)
    .bind(pr_number)
    .bind(file_path)
    .bind(line as i32)
    .fetch_one(pool.as_pg())
    .await?;
    Ok(count > 0)
}

/// Filter findings to only those on new code lines.
///
/// Findings that land on pre-existing (non-new) lines are recorded into the
/// baseline and then suppressed. This stops detectors like `boilerplate` from
/// re-flagging the same issue every time a file is touched.
///
/// `repo_full_name` is the `owner/repo` slug; `pr_number` is the PR id.
pub async fn filter_new_code_findings(
    pool: &DbPool,
    repo_full_name: &str,
    pr_number: i64,
    findings: &[Finding],
) -> Result<Vec<Finding>> {
    let mut out = Vec::new();
    for f in findings {
        let is_new = is_new_code_line(pool, repo_full_name, pr_number, &f.file, f.line).await?;
        if is_new {
            out.push(f.clone());
            continue;
        }
        let fp = f.fingerprint();
        if is_baseline(pool, repo_full_name, &fp).await? {
            // Already known pre-existing issue: suppress.
            continue;
        }
        // First time we see this issue on old code: record it silently.
        record_baseline(pool, repo_full_name, &fp).await?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_hunk() {
        let patch = "@@ -1,3 +1,5 @@\n line1\n-line2\n+line2new\n+line3new\n line4\n";
        let lines = parse_patch_changed_lines(patch);
        assert!(lines.contains(&2));
        assert!(lines.contains(&3));
        assert!(!lines.contains(&1));
        assert!(!lines.contains(&4));
    }

    #[test]
    fn parse_no_newline_marker() {
        let patch = "@@ -1 +1,2 @@\n-old\n+new\n\\ No newline at end of file\n";
        let lines = parse_patch_changed_lines(patch);
        assert!(lines.contains(&1));
    }

    #[test]
    fn parse_multiple_hunks() {
        let patch = "@@ -1,2 +1,3 @@\n a\n+b\n c\n@@ -10,2 +11,3 @@\n x\n+y\n z\n";
        let lines = parse_patch_changed_lines(patch);
        assert!(lines.contains(&2));
        assert!(lines.contains(&12));
    }
}
