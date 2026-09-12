//! Nudge detector: flags a PR that adds/changes functions but touches no test files.
//!
//! Deliberately a heuristic, not a coverage tool: it does not diff old vs new
//! symbols, it just checks whether a changed non-test file has any
//! function-like node at all, and whether any changed file looks like a test.

use crate::detectors::Finding;
use crate::index::extract::{extract_file, SYMBOL_FUNCTION, SYMBOL_METHOD};
use crate::parser::ParsedFile;

pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    if parsed_files.iter().any(is_test_file) {
        return Vec::new();
    }

    let changed_with_functions: Vec<&ParsedFile> = parsed_files
        .iter()
        .filter(|f| file_has_new_function(f))
        .collect();

    let Some(first) = changed_with_functions.first() else {
        return Vec::new();
    };

    let n = changed_with_functions.len();
    let plural = if n == 1 { "" } else { "s" };
    vec![Finding {
        detector: "test-coverage".to_string(),
        severity: "warning",
        file: first.path.clone(),
        line: 0,
        column: 0,
        message: format!(
            "This PR changes {n} file{plural} with new/modified functions but doesn't touch any test files — consider adding test coverage."
        ),
        suggestion: Some(
            "Add or update tests covering the changed functions in this PR.".to_string(),
        ),
        evidence: None,
        codemod: None,
        confidence: None,
        judge_rationale: None,
        reachability: None,
    }]
}

fn is_test_file(file: &ParsedFile) -> bool {
    let path = file.path.to_lowercase().replace('\\', "/");
    path.contains("/test/")
        || path.contains("/tests/")
        || path.starts_with("test/")
        || path.starts_with("tests/")
        || path.contains("_test.")
        || path.contains(".test.")
        || path.contains(".spec.")
        || path
            .rsplit('/')
            .next()
            .is_some_and(|f| f.starts_with("test_"))
        || file.raw_content.contains("#[cfg(test)]")
}

fn file_has_new_function(file: &ParsedFile) -> bool {
    let Some(index) = extract_file(&file.path, &file.raw_content) else {
        return false;
    };
    index
        .symbols
        .iter()
        .any(|s| s.kind == SYMBOL_FUNCTION || s.kind == SYMBOL_METHOD)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    fn parse(name: &str, content: &str) -> ParsedFile {
        parse_file(name, content).unwrap()
    }

    #[test]
    fn flags_new_function_without_tests() {
        let files = [parse("src/lib.rs", "pub fn added() -> i32 { 42 }\n")];
        let findings = detect(&files);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "warning");
    }

    #[test]
    fn no_finding_when_test_file_touched() {
        let files = [
            parse("src/lib.rs", "pub fn added() -> i32 { 42 }\n"),
            parse("src/lib_test.rs", "#[test]\nfn t() {}\n"),
        ];
        let findings = detect(&files);
        assert!(findings.is_empty());
    }

    #[test]
    fn no_finding_when_no_functions_changed() {
        let files = [parse("README.md", "# hello\n")];
        let findings = detect(&files);
        assert!(findings.is_empty());
    }

    #[test]
    fn detects_rust_cfg_test_module_as_test_file() {
        let files = [
            parse("src/lib.rs", "pub fn added() -> i32 { 42 }\n"),
            parse(
                "src/other.rs",
                "fn helper() {}\n\n#[cfg(test)]\nmod tests {}\n",
            ),
        ];
        let findings = detect(&files);
        assert!(findings.is_empty());
    }
}
