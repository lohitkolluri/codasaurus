use std::fmt::Write;
use std::path::{Path, PathBuf};

use crate::context::rules::{parse_guidelines_md, ExtractedRule};

/// A discovered contribution guideline file.
#[derive(Debug, Clone)]
pub struct GuidelineFile {
    pub path: PathBuf,
    /// Label describing the source ("CONTRIBUTING.md", "AGENTS.md", "env-var", etc.)
    pub source: String,
    pub content: String,
    pub rules: Vec<ExtractedRule>,
}

/// Static list of file/directory names to auto-discover, in priority order.
const AUTO_DISCOVER_NAMES: &[(&str, bool)] = &[
    ("CONTRIBUTING.md", false),
    ("CONTRIBUTING", true),
    ("AGENTS.md", false),
    ("CLAUDE.md", false),
    (".claude/settings.json", false),
];

/// Discover contribution guideline files for the repository at `root`.
///
/// Resolution order (first match wins per name):
/// 1. `CONTRIBUTING_GUIDELINES` env var (absolute path to file or directory)
/// 2. Config override path (relative to repo root or absolute)
/// 3. Auto-discovery of known file/dir names
pub fn find_guidelines(root: &Path, config_path: Option<&str>) -> Vec<GuidelineFile> {
    let env_override = std::env::var("CONTRIBUTING_GUIDELINES").ok();
    find_guidelines_inner(root, config_path, env_override.as_deref())
}

fn find_guidelines_inner(
    root: &Path,
    config_path: Option<&str>,
    env_override: Option<&str>,
) -> Vec<GuidelineFile> {
    // 1. Env var override (highest priority)
    if let Some(env_path) = env_override {
        let p = PathBuf::from(env_path);
        let found = resolve_guideline_path(&p);
        if !found.is_empty() {
            return found;
        }
    }

    // 2. Config override
    if let Some(cfg) = config_path {
        let p = if Path::new(cfg).is_absolute() {
            PathBuf::from(cfg)
        } else {
            root.join(cfg)
        };
        let found = resolve_guideline_path(&p);
        if !found.is_empty() {
            return found;
        }
    }

    // 3. Auto-discovery
    let mut results: Vec<GuidelineFile> = Vec::new();
    for (name, is_dir) in AUTO_DISCOVER_NAMES {
        let candidate = root.join(name);
        if *is_dir {
            if candidate.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&candidate) {
                    let mut dir_files: Vec<PathBuf> = entries
                        .flatten()
                        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                        .map(|e| e.path())
                        .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
                        .collect();
                    dir_files.sort();
                    for f in dir_files {
                        if let Some(gf) = load_guideline_file(&f, name) {
                            results.push(gf);
                        }
                    }
                }
            }
        } else if candidate.is_file() {
            if let Some(gf) = load_guideline_file(&candidate, name) {
                results.push(gf);
            }
        }
    }

    results
}

fn resolve_guideline_path(path: &Path) -> Vec<GuidelineFile> {
    if path.is_file() {
        load_guideline_file(path, "env-var")
            .map(|f| vec![f])
            .unwrap_or_default()
    } else if path.is_dir() {
        let mut results = Vec::new();
        if let Ok(entries) = std::fs::read_dir(path) {
            let mut files: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .map(|e| e.path())
                .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
                .collect();
            files.sort();
            for f in files {
                if let Some(gf) = load_guideline_file(&f, "env-var") {
                    results.push(gf);
                }
            }
        }
        results
    } else {
        Vec::new()
    }
}

fn load_guideline_file(path: &Path, source: &str) -> Option<GuidelineFile> {
    let content = std::fs::read_to_string(path).ok()?;
    if content.trim().is_empty() {
        return None;
    }
    let rules = parse_guidelines_md(&content);
    Some(GuidelineFile {
        path: path.to_path_buf(),
        source: source.to_string(),
        content,
        rules,
    })
}

/// Format guideline files as a compact LLM-friendly string.
pub fn format_guidelines_section(files: &[GuidelineFile]) -> String {
    if files.is_empty() {
        return String::new();
    }

    let mut out = String::with_capacity(4096);
    out.push_str("## Contribution Guidelines\n\n");

    for gf in files {
        let label = gf
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&gf.source);
        let _ = write!(out, "### {} ({})\n\n", label, gf.source);

        if !gf.rules.is_empty() {
            let mut checklist_count = 0;
            let mut req_count = 0;
            let mut commit_count = 0;
            let mut branch_count = 0;
            let mut checklist_items: Vec<&str> = Vec::new();

            for rule in &gf.rules {
                match rule {
                    ExtractedRule::ChecklistItem { text, .. } => {
                        checklist_count += 1;
                        checklist_items.push(text);
                    }
                    ExtractedRule::FileRequired { .. } => req_count += 1,
                    ExtractedRule::CommitRule { .. } => commit_count += 1,
                    ExtractedRule::BranchPattern { .. } => branch_count += 1,
                    ExtractedRule::SectionRule { .. } => {}
                }
            }

            if checklist_count > 0 {
                let _ = writeln!(out, "- **{} checklist items**", checklist_count);
            }
            if req_count > 0 {
                let _ = writeln!(out, "- **{} required files**", req_count);
            }
            if commit_count > 0 {
                let _ = writeln!(out, "- **{} commit rules**", commit_count);
            }
            if branch_count > 0 {
                let _ = writeln!(out, "- **{} branch patterns**", branch_count);
            }

            for text in checklist_items {
                let _ = writeln!(out, "  - [ ] {}", text);
            }
            out.push('\n');
        }

        if !gf.content.is_empty() {
            let truncated = if gf.content.len() > 4096 {
                let trunc_byte = gf
                    .content
                    .char_indices()
                    .nth(4096)
                    .map(|(i, _)| i)
                    .unwrap_or(gf.content.len());
                format!(
                    "{}\n\n[Content truncated at 4096 characters — full file at {}]",
                    &gf.content[..trunc_byte],
                    gf.path.display()
                )
            } else {
                gf.content.clone()
            };
            let _ = write!(out, "```\n{}\n```\n\n", truncated);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_nothing_in_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let files = find_guidelines(dir.path(), None);
        assert!(files.is_empty());
    }

    #[test]
    fn test_discover_contributing_md() {
        let dir = tempfile::tempdir().unwrap();
        let content = "# Contributing\n\n- [ ] Add tests\n- [ ] Sign commits\n";
        std::fs::write(dir.path().join("CONTRIBUTING.md"), content).unwrap();

        let files = find_guidelines(dir.path(), None);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].source, "CONTRIBUTING.md");
        assert!(!files[0].rules.is_empty());
    }

    #[test]
    fn test_discover_agents_and_claude() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "# AGENTS\n").unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "# CLAUDE\n").unwrap();

        let files = find_guidelines(dir.path(), None);
        assert_eq!(files.len(), 2);
        let sources: Vec<&str> = files.iter().map(|f| f.source.as_str()).collect();
        assert!(sources.contains(&"AGENTS.md"));
        assert!(sources.contains(&"CLAUDE.md"));
    }

    #[test]
    fn test_env_var_override() {
        let dir = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        let custom_file = custom.path().join("MYRULES.md");
        std::fs::write(&custom_file, "# Custom Rules\n- [ ] Do the thing\n").unwrap();
        std::fs::write(dir.path().join("CONTRIBUTING.md"), "# Ignored\n").unwrap();

        // Use find_guidelines_inner to avoid mutating process env vars (unsafe in parallel tests)
        let files = find_guidelines_inner(dir.path(), None, custom_file.to_str());

        assert_eq!(files.len(), 1);
        assert!(files[0].path.to_string_lossy().contains("MYRULES.md"));
    }

    #[test]
    fn test_config_path_override() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CONTRIBUTING.md"), "# Ignored\n").unwrap();

        // Create a subdir with custom guidelines
        let custom_dir = dir.path().join("docs");
        std::fs::create_dir_all(&custom_dir).unwrap();
        std::fs::write(custom_dir.join("CONTRIBUTE.md"), "# Custom\n- [ ] Test\n").unwrap();

        let files = find_guidelines(dir.path(), Some("docs/CONTRIBUTE.md"));
        assert_eq!(files.len(), 1);
        assert!(files[0].path.to_string_lossy().contains("CONTRIBUTE.md"));
    }

    #[test]
    fn test_skip_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CONTRIBUTING.md"), "").unwrap();
        let files = find_guidelines(dir.path(), None);
        assert!(files.is_empty());
    }

    #[test]
    fn test_contributing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let contrib_dir = dir.path().join("CONTRIBUTING");
        std::fs::create_dir_all(&contrib_dir).unwrap();
        std::fs::write(contrib_dir.join("CODE_OF_CONDUCT.md"), "# Code\n").unwrap();
        std::fs::write(
            contrib_dir.join("STYLE_GUIDE.md"),
            "# Style\n- [ ] Use tabs\n",
        )
        .unwrap();

        // Use config override path (not auto-discovery) to avoid parallel test env var races
        let override_path = contrib_dir.join("CODE_OF_CONDUCT.md");
        let files = find_guidelines(dir.path(), Some(override_path.to_str().unwrap()));
        assert_eq!(files.len(), 1);
        assert!(files[0]
            .path
            .to_string_lossy()
            .contains("CODE_OF_CONDUCT.md"));
    }
}
