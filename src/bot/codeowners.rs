//! Minimal CODEOWNERS parser for reviewer suggestions.

use std::collections::BTreeSet;

/// A single CODEOWNERS rule: glob-ish path pattern → owners (@user or @org/team).
#[derive(Debug, Clone)]
pub struct CodeOwnerRule {
    pub pattern: String,
    pub owners: Vec<String>,
}

/// Parse CODEOWNERS file content into rules (last matching rule wins, GitHub style).
pub fn parse_codeowners(content: &str) -> Vec<CodeOwnerRule> {
    let mut rules = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(pattern) = parts.next() else {
            continue;
        };
        let owners: Vec<String> = parts
            .filter(|p| p.starts_with('@'))
            .map(|p| p.trim_start_matches('@').to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if owners.is_empty() {
            continue;
        }
        rules.push(CodeOwnerRule {
            pattern: pattern.to_string(),
            owners,
        });
    }
    rules
}

/// Owners for changed paths (union; pattern match is suffix/prefix/contains style).
pub fn owners_for_paths(rules: &[CodeOwnerRule], paths: &[String]) -> Vec<String> {
    let mut owners = BTreeSet::new();
    for path in paths {
        let mut matched: Option<&[String]> = None;
        for rule in rules {
            if path_matches(&rule.pattern, path) {
                matched = Some(&rule.owners);
            }
        }
        if let Some(os) = matched {
            for o in os {
                owners.insert(o.clone());
            }
        }
    }
    owners.into_iter().collect()
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let pat = pattern.trim_start_matches('/');
    let path = path.trim_start_matches('/');
    if pat == "*" || pat == "**" {
        return true;
    }
    if let Some(dir) = pat.strip_suffix("/**") {
        return path == dir || path.starts_with(&format!("{dir}/"));
    }
    if let Some(prefix) = pat.strip_suffix("/*") {
        if let Some(rest) = path.strip_prefix(prefix) {
            let rest = rest.trim_start_matches('/');
            return !rest.is_empty() && !rest.contains('/');
        }
        return false;
    }
    if let Some(suffix) = pat.strip_prefix("*.") {
        return path.ends_with(&format!(".{suffix}"))
            || path.rsplit('/').next().is_some_and(|f| f.ends_with(&format!(".{suffix}")));
    }
    if pat.ends_with('/') {
        return path.starts_with(pat) || path.starts_with(pat.trim_end_matches('/'));
    }
    path == pat || path.ends_with(&format!("/{pat}")) || path.starts_with(&format!("{pat}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_matches() {
        let rules = parse_codeowners(
            r#"
# comment
*.rs @rust-team
/src/bot/ @bot-owners @alice
docs/ @docs
"#,
        );
        assert_eq!(rules.len(), 3);
        let owners = owners_for_paths(
            &rules,
            &["src/bot/review.rs".into(), "lib.rs".into()],
        );
        assert!(owners.iter().any(|o| o == "bot-owners" || o == "alice"));
        assert!(owners.iter().any(|o| o == "rust-team"));
    }

    #[test]
    fn last_match_wins_per_path() {
        let rules = parse_codeowners(
            "* @everyone\nsrc/ @src-team\nsrc/bot/ @bot-team\n",
        );
        let owners = owners_for_paths(&rules, &["src/bot/mod.rs".into()]);
        assert_eq!(owners, vec!["bot-team".to_string()]);
    }
}
