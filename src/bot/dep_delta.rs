//! Manifest dependency delta (mini-SBOM) for walkthroughs.

use crate::dep_parser;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
pub struct DepDelta {
    pub path: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// Diff dependency sets between old and new manifest content.
pub fn diff_manifest(path: &str, old: &str, new: &str) -> Option<DepDelta> {
    if old.trim().is_empty() && !new.trim().is_empty() {
        let added = extract_deps(path, new).unwrap_or_default();
        if added.is_empty() {
            return None;
        }
        return Some(DepDelta {
            path: path.to_string(),
            added,
            removed: vec![],
        });
    }
    let old_pkgs = extract_deps(path, old)?;
    let new_pkgs = extract_deps(path, new)?;
    let old_set: BTreeSet<_> = old_pkgs.into_iter().collect();
    let new_set: BTreeSet<_> = new_pkgs.into_iter().collect();
    let added: Vec<_> = new_set.difference(&old_set).cloned().collect();
    let removed: Vec<_> = old_set.difference(&new_set).cloned().collect();
    if added.is_empty() && removed.is_empty() {
        return None;
    }
    Some(DepDelta {
        path: path.to_string(),
        added,
        removed,
    })
}

fn extract_deps(path: &str, content: &str) -> Option<Vec<String>> {
    let lower = path.to_lowercase();
    let pkgs = if lower.ends_with("package.json") {
        dep_parser::extract_npm_deps(content)
    } else if lower.ends_with("cargo.toml") {
        dep_parser::extract_cargo_deps(content)
    } else if lower.ends_with("requirements.txt") || lower.ends_with("requirements-dev.txt") {
        dep_parser::extract_requirements_deps(content)
    } else if lower.ends_with("pyproject.toml") {
        dep_parser::extract_pyproject_deps(content)
    } else if lower.ends_with("go.mod") {
        dep_parser::extract_go_mod_deps(content)
    } else {
        return None;
    };
    Some(pkgs)
}

pub fn is_manifest_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with("package.json")
        || lower.ends_with("cargo.toml")
        || lower.ends_with("requirements.txt")
        || lower.ends_with("requirements-dev.txt")
        || lower.ends_with("pyproject.toml")
        || lower.ends_with("go.mod")
}

/// Best-effort delta from unified diff patch of a manifest (added lines only when old unavailable).
pub fn delta_from_patch(path: &str, patch: &str) -> Option<DepDelta> {
    if !is_manifest_path(path) || patch.is_empty() {
        return None;
    }
    let mut added_lines = String::new();
    let mut removed_lines = String::new();
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix('+').filter(|l| !l.starts_with("++")) {
            added_lines.push_str(rest);
            added_lines.push('\n');
        } else if let Some(rest) = line.strip_prefix('-').filter(|l| !l.starts_with("--")) {
            removed_lines.push_str(rest);
            removed_lines.push('\n');
        }
    }
    // Reconstruct approximate manifests from +/- lines is lossy; prefer full-file when available.
    // Fallback: parse added hunk as if it were a mini-manifest fragment for cargo/npm keys.
    if path.to_lowercase().ends_with("package.json") {
        // Can't parse fragments — require full content path in caller.
        return None;
    }
    let added = extract_deps(path, &added_lines).unwrap_or_default();
    let removed = extract_deps(path, &removed_lines).unwrap_or_default();
    let added_set: BTreeSet<_> = added.into_iter().collect();
    let removed_set: BTreeSet<_> = removed.into_iter().collect();
    // Packages in both are version bumps — treat as neither add nor remove for simplicity,
    // or list as updated by excluding intersection from both.
    let both: BTreeSet<_> = added_set.intersection(&removed_set).cloned().collect();
    let added: Vec<_> = added_set.difference(&both).cloned().collect();
    let removed: Vec<_> = removed_set.difference(&both).cloned().collect();
    if added.is_empty() && removed.is_empty() && both.is_empty() {
        return None;
    }
    let mut d = DepDelta {
        path: path.to_string(),
        added,
        removed,
    };
    // Surface updates as "added" with note — keep simple: append updated names to added list as `name (updated)`
    for u in both {
        d.added.push(format!("{u} (updated)"));
    }
    Some(d)
}

pub fn dep_delta_markdown(deltas: &[DepDelta], vuln_packages: &[String]) -> String {
    if deltas.is_empty() {
        return String::new();
    }
    let mut out = String::from("**Dependency delta**\n\n");
    for d in deltas {
        out.push_str(&format!("`{}`\n", d.path));
        for p in &d.added {
            let flag = if vuln_packages.iter().any(|v| p.contains(v)) {
                " · vuln"
            } else {
                ""
            };
            let kind = if p.contains("(updated)") {
                "updated"
            } else {
                "added"
            };
            let name = p.replace(" (updated)", "");
            out.push_str(&format!("- {kind} `{name}`{flag}\n"));
        }
        for p in &d.removed {
            out.push_str(&format!("- removed `{p}`\n"));
        }
        out.push('\n');
    }
    out.push_str("<sub>From manifest diffs only.</sub>\n\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_toml_delta() {
        let old = "[dependencies]\nserde = \"1\"\n";
        let new = "[dependencies]\nserde = \"1\"\ntokio = \"1\"\n";
        let d = diff_manifest("Cargo.toml", old, new).unwrap();
        assert!(d.added.iter().any(|p| p == "tokio"));
    }
}
