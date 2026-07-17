use once_cell::sync::Lazy;
use regex::Regex;

static CHECKBOX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*[-*]\s+\[([ xX])\]\s+(.+)$").unwrap());
static HEADING_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^#{2,4}\s+(.+)$").unwrap());
static BRANCH_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)branch\s*(naming|pattern|name|format)?\s*[:]\s*(.+?)(?:[,.\n]|$)").unwrap());
static BRANCH_INLINE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(branches?\s+(?:must|should)\s+(?:start|begin)\s+with\s+`([^`]+)`)").unwrap());
static DCO_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(signed.off.by|DCO|developer.certificate.of.origin)").unwrap());
static CONVENTIONAL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(conventional\s*commits?|commit\s*(message)?\s*(convention|format|style))").unwrap());
static FILE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(?:required|must\s+have|must\s+include)\s+`([^`]+)`").unwrap());

/// A structured rule extracted from a contribution guideline file.
#[derive(Debug, Clone)]
pub enum ExtractedRule {
    /// A markdown checklist item from `- [ ]` / `- [x]`
    ChecklistItem { text: String, checked: bool },
    /// A required file path mentioned in the guidelines
    FileRequired { path: String },
    /// A branch naming pattern mentioned
    BranchPattern { pattern: String },
    /// A commit message convention (conventional commits, sign-off, etc.)
    CommitRule { description: String },
    /// A generic parsed section with heading and content
    SectionRule { heading: String, text: String },
}

/// Parses a markdown guidelines document and extracts structured rules.
///
/// Extracts:
/// - Checklist items (`- [ ]` / `- [x]`)
/// - File requirements from "## Requirements" / "## Prerequisites" sections
/// - Commit conventions from "## Commit Convention" / "## Commit Messages" sections
/// - Branch patterns from content mentioning "branch"
/// - DCO / sign-off requirements
/// - Generic heading-based sections
pub fn parse_guidelines_md(content: &str) -> Vec<ExtractedRule> {
    let mut rules: Vec<ExtractedRule> = Vec::new();
    let mut in_requirements = false;
    let mut in_commit_section = false;

    for line in content.lines() {
        if let Some(caps) = HEADING_RE.captures(line) {
            let heading = caps[1].trim().to_string();

            let hl = heading.to_lowercase();
            in_requirements = hl.contains("requirement")
                || hl.contains("prerequisite")
                || hl.contains("checklist")
                || hl.contains("definition of done");
            in_commit_section = hl.contains("commit")
                || hl.contains("commit message")
                || hl.contains("commit convention");

            rules.push(ExtractedRule::SectionRule {
                heading: heading.clone(),
                text: String::new(), // populated below
            });
            continue;
        }

        if let Some(ExtractedRule::SectionRule { text, .. }) = rules.last_mut() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(line);
        }

        // 1. Checklist items
        if let Some(caps) = CHECKBOX_RE.captures(line) {
            let checked = matches!(&caps[1], "x" | "X");
            rules.push(ExtractedRule::ChecklistItem {
                text: caps[2].trim().to_string(),
                checked,
            });
            continue;
        }

        // 2. Required files
        if let Some(caps) = FILE_RE.captures(line) {
            rules.push(ExtractedRule::FileRequired {
                path: caps[1].to_string(),
            });
            continue;
        }

        // 3. Commit conventions (check whole line, not just section header)
        if CONVENTIONAL_RE.is_match(line) {
            rules.push(ExtractedRule::CommitRule {
                description: line.trim().to_string(),
            });
            continue;
        }

        // 4. DCO / sign-off
        if DCO_RE.is_match(line) {
            rules.push(ExtractedRule::CommitRule {
                description: line.trim().to_string(),
            });
            continue;
        }

        // 5. Branch naming pattern (explicit `branch:` prefix)
        if let Some(caps) = BRANCH_RE.captures(line) {
            let pattern = caps[2].trim().to_string();
            if !pattern.is_empty() {
                rules.push(ExtractedRule::BranchPattern { pattern });
                continue;
            }
        }
        if let Some(caps) = BRANCH_INLINE_RE.captures(line) {
            let pattern = caps[2].to_string();
            if !pattern.is_empty() {
                rules.push(ExtractedRule::BranchPattern { pattern });
            }
            continue;
        }

        // 6. In requirements section: capture bullet points as checklist items
        if in_requirements {
            let trimmed = line.trim();
            if let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                let item = item.trim();
                if !item.is_empty() && !item.starts_with('[') {
                    rules.push(ExtractedRule::ChecklistItem {
                        text: item.to_string(),
                        checked: false,
                    });
                }
            }
        }

        // 7. In commit section: capture any bullet as a commit rule
        if in_commit_section {
            let trimmed = line.trim();
            if let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                let item = item.trim();
                if !item.is_empty() && !item.starts_with('[') {
                    rules.push(ExtractedRule::CommitRule {
                        description: item.to_string(),
                    });
                }
            }
        }
    }

    // Dedup by type + text to avoid rule explosion
    dedup_rules(&mut rules);
    rules
}

fn dedup_rules(rules: &mut Vec<ExtractedRule>) {
    let mut seen = std::collections::HashSet::new();
    rules.retain(|r| {
        let key = match r {
            ExtractedRule::ChecklistItem { text, .. } => format!("ci:{}", text),
            ExtractedRule::FileRequired { path } => format!("fr:{}", path),
            ExtractedRule::BranchPattern { pattern } => format!("bp:{}", pattern),
            ExtractedRule::CommitRule { description } => format!("cr:{}", description),
            ExtractedRule::SectionRule { heading, .. } => format!("sr:{}", heading),
        };
        seen.insert(key)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_checklist_items() {
        let md = "- [ ] Add tests\n- [x] Sign commits\n- [ ] Update docs\n";
        let rules = parse_guidelines_md(md);
        let items: Vec<&ExtractedRule> = rules.iter().filter(|r| matches!(r, ExtractedRule::ChecklistItem { .. })).collect();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn test_parse_commit_convention() {
        let md = "## Commit Convention\nUse conventional commits: `type(scope): description`\n";
        let rules = parse_guidelines_md(md);
        let commit_rules: Vec<&ExtractedRule> = rules.iter().filter(|r| matches!(r, ExtractedRule::CommitRule { .. })).collect();
        assert!(!commit_rules.is_empty());
    }

    #[test]
    fn test_parse_dco() {
        let md = "All commits must be signed-off-by per DCO guidelines.";
        let rules = parse_guidelines_md(md);
        assert!(rules.iter().any(|r| matches!(r, ExtractedRule::CommitRule { description } if description.to_lowercase().contains("dco"))));
    }

    #[test]
    fn test_parse_branch_pattern() {
        let md = "Branch naming: `feat/`, `fix/`, or `chore/`";
        let rules = parse_guidelines_md(md);
        let branches: Vec<&ExtractedRule> = rules.iter().filter(|r| matches!(r, ExtractedRule::BranchPattern { .. })).collect();
        assert!(!branches.is_empty());
    }

    #[test]
    fn test_parse_required_files() {
        let md = "Your PR must include `CHANGELOG.md` and `README.md`.";
        let rules = parse_guidelines_md(md);
        let files: Vec<&ExtractedRule> = rules.iter().filter(|r| matches!(r, ExtractedRule::FileRequired { .. })).collect();
        assert!(!files.is_empty());
    }

    #[test]
    fn test_parse_sections() {
        let md = "## Testing\nRun tests with `cargo test`.\n## Docs\nUpdate docs.";
        let rules = parse_guidelines_md(md);
        let sections: Vec<&ExtractedRule> = rules.iter().filter(|r| matches!(r, ExtractedRule::SectionRule { .. })).collect();
        assert_eq!(sections.len(), 2);
    }

    #[test]
    fn test_dedup_rules() {
        let md = "- [ ] Add tests\n- [ ] Add tests\n## Commit Convention\nconventional commits\nconventional commits\n";
        let rules = parse_guidelines_md(md);
        let checklists: Vec<&ExtractedRule> = rules.iter().filter(|r| matches!(r, ExtractedRule::ChecklistItem { text, .. } if text == "Add tests")).collect();
        assert_eq!(checklists.len(), 1);
    }

    #[test]
    fn test_requirements_section_lists() {
        let md = "## Requirements\n- Node.js 18+\n- Rust 1.70+\n- Docker";
        let rules = parse_guidelines_md(md);
        let items: Vec<&ExtractedRule> = rules.iter().filter(|r| matches!(r, ExtractedRule::ChecklistItem { .. })).collect();
        assert!(!items.is_empty());
    }

    #[test]
    fn test_branch_inline_rule() {
        let md = "Branches must start with `feat/` or `fix/`.";
        let rules = parse_guidelines_md(md);
        let branches: Vec<&ExtractedRule> = rules.iter().filter(|r| matches!(r, ExtractedRule::BranchPattern { .. })).collect();
        assert!(!branches.is_empty());
    }

    #[test]
    fn test_empty_content() {
        let rules = parse_guidelines_md("");
        assert!(rules.is_empty());
    }

    #[test]
    fn test_noise_only() {
        let md = "Just some random text without any recognizable patterns.";
        let rules = parse_guidelines_md(md);
        // Should still get a SectionRule from the non-heading text
        // Actually without any heading, there won't be sections
        assert!(rules.is_empty());
    }
}
