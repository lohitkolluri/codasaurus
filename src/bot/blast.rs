//! Bounded blast-radius estimate from import edges in the PR diff.

use crate::parser::ParsedFile;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct BlastReport {
    pub high_risk_paths: Vec<String>,
    pub fan_in: Vec<(String, usize)>,
    pub score: u8,
}

/// Estimate blast radius from imports among parsed PR files (no full-repo index).
pub fn estimate_blast_radius(files: &[ParsedFile], changed_paths: &[String]) -> BlastReport {
    let mut importers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for f in files {
        for imp in &f.imports {
            let target = normalize_import(&imp.name, &f.path);
            if target.is_empty() {
                continue;
            }
            importers.entry(target).or_default().insert(f.path.clone());
        }
    }

    let mut fan_in: Vec<(String, usize)> = importers
        .iter()
        .map(|(k, v)| (k.clone(), v.len()))
        .filter(|(_, n)| *n >= 1)
        .collect();
    fan_in.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    fan_in.truncate(12);

    let high_risk_paths: Vec<String> = changed_paths
        .iter()
        .filter(|p| is_high_fan_in_path(p))
        .take(8)
        .cloned()
        .collect();

    let mut score = 0u8;
    if !high_risk_paths.is_empty() {
        score = score.saturating_add(40);
    }
    if let Some((_, n)) = fan_in.first() {
        if *n >= 5 {
            score = score.saturating_add(35);
        } else if *n >= 2 {
            score = score.saturating_add(20);
        }
    }
    if changed_paths.len() > 25 {
        score = score.saturating_add(15);
    }
    score = score.min(100);

    BlastReport {
        high_risk_paths,
        fan_in,
        score,
    }
}

fn normalize_import(name: &str, from_path: &str) -> String {
    let n = name.trim();
    if n.starts_with('.') || n.starts_with('/') {
        // Relative — keep as-is for matching path stems
        return format!("{from_path}::{n}");
    }
    n.split('/').next().unwrap_or(n).to_string()
}

fn is_high_fan_in_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("auth")
        || lower.contains("middleware")
        || lower.contains("shared")
        || lower.contains("common/")
        || lower.contains("/types/")
        || lower.contains("schema")
        || lower.ends_with("mod.rs")
        || lower.contains("api/client")
}

pub fn blast_markdown(report: &BlastReport) -> String {
    if report.score == 0 && report.fan_in.is_empty() && report.high_risk_paths.is_empty() {
        return String::new();
    }
    let level = match report.score {
        0..=24 => "low",
        25..=59 => "moderate",
        60..=84 => "high",
        _ => "critical",
    };
    let mut out = format!("**Blast radius:** `{level}` ({}/100)", report.score);
    if let Some(path) = report.high_risk_paths.first() {
        out.push_str(&format!(" · sensitive: `{path}`"));
        if report.high_risk_paths.len() > 1 {
            out.push_str(&format!(" (+{})", report.high_risk_paths.len() - 1));
        }
    }
    out.push_str("\n\n");
    if !report.fan_in.is_empty() || report.high_risk_paths.len() > 1 {
        out.push_str("<details>\n<summary>Blast details</summary>\n\n");
        if report.high_risk_paths.len() > 1 {
            out.push_str("High-sensitivity paths:\n\n");
            for p in &report.high_risk_paths {
                out.push_str(&format!("- `{p}`\n"));
            }
            out.push('\n');
        }
        if !report.fan_in.is_empty() {
            out.push_str("| Import / module | Importers in this PR |\n| --- | ---: |\n");
            for (name, n) in report.fan_in.iter().take(6) {
                out.push_str(&format!("| `{name}` | {n} |\n"));
            }
            out.push('\n');
        }
        out.push_str("<sub>Bounded estimate from PR imports only.</sub>\n\n</details>\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Import, ParsedFile};

    #[test]
    fn scores_auth_paths() {
        let files = vec![ParsedFile {
            path: "src/app.ts".into(),
            language: "typescript".into(),
            lines: vec![],
            imports: vec![Import {
                name: "./auth".into(),
                line: 1,
                column: 0,
            }],
            raw_content: String::new(),
        }];
        let report = estimate_blast_radius(&files, &["src/auth/middleware.ts".into()]);
        assert!(!report.high_risk_paths.is_empty());
        assert!(report.score > 0);
    }
}
