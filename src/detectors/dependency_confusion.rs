use crate::detectors::Finding;
use crate::parser::ParsedFile;
use crate::registry;

/// Flags scoped/internal-looking dependencies (`@company/foo`) that also resolve
/// on the public npm registry — the classic dependency-confusion attack surface:
/// an attacker publishes a public package under your private scope name and it
/// gets pulled instead of your internal one.
pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for file in parsed_files {
        if !file.path.to_lowercase().ends_with("package.json") {
            continue;
        }
        if crate::detectors::is_test_or_fixture_path(&file.path) {
            continue;
        }

        for dep in crate::dep_parser::extract_npm_deps(&file.raw_content) {
            if !dep.starts_with('@') {
                continue;
            }
            if let Ok(Some(true)) = registry::check_package("npm", &dep) {
                findings.push(Finding {
                    detector: "dependency-confusion".to_string(),
                    severity: "warning",
                    file: file.path.clone(),
                    line: 0,
                    column: 0,
                    message: format!(
                        "Scoped dependency `{dep}` resolves on the public npm registry. If `{dep}` is meant to be an internal/private package, this could be a dependency-confusion attack — the public package may be malicious and could shadow your private one."
                    ),
                    suggestion: Some(format!(
                        "Verify `{dep}` on https://www.npmjs.com/package/{dep} is the package you expect, and pin installs to your private registry (npm scope config or a lockfile with an explicit registry)."
                    )),
                    evidence: Some(dep.clone()),
                    codemod: None,
                    confidence: None,
                    judge_rationale: None,
                    reachability: None,
                });
            }
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    #[test]
    fn flags_scoped_dep_that_exists_publicly() {
        let file = parse_file(
            "package.json",
            r#"{"dependencies": {"@types/node": "^20.0.0"}}"#,
        )
        .unwrap();
        let findings = detect(&[file]);
        assert!(
            findings.iter().any(|f| f.message.contains("@types/node")),
            "expected a dependency-confusion finding for a publicly-resolving scoped package"
        );
    }

    #[test]
    fn ignores_unscoped_deps() {
        let file = parse_file("package.json", r#"{"dependencies": {"lodash": "^4.0.0"}}"#).unwrap();
        assert!(detect(&[file]).is_empty());
    }
}
