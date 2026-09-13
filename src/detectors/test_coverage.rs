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

/// Guess sibling test file path(s) for a source file, by language convention.
/// Used only to decide whether a plausible test file already exists, not to
/// generate paths blindly.
pub fn sibling_test_candidates(path: &str) -> Vec<String> {
    let Some((dir, file)) = path.rsplit_once('/') else {
        return sibling_test_candidates(&format!("./{path}"))
            .into_iter()
            .map(|p| p.trim_start_matches("./").to_string())
            .collect();
    };
    let Some((stem, ext)) = file.rsplit_once('.') else {
        return Vec::new();
    };
    match ext {
        "py" => vec![
            format!("{dir}/test_{stem}.py"),
            format!("{dir}/{stem}_test.py"),
        ],
        "ts" | "tsx" | "js" | "jsx" => vec![
            format!("{dir}/{stem}.test.{ext}"),
            format!("{dir}/{stem}.spec.{ext}"),
            format!("{dir}/__tests__/{stem}.test.{ext}"),
        ],
        "go" => vec![format!("{dir}/{stem}_test.go")],
        "rs" => vec![path.to_string()], // same-file #[cfg(test)] mod tests
        _ => Vec::new(),
    }
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
    fn sibling_test_candidates_cover_major_languages() {
        assert!(sibling_test_candidates("src/foo.py").contains(&"src/test_foo.py".to_string()));
        assert!(sibling_test_candidates("src/foo.ts").contains(&"src/foo.test.ts".to_string()));
        assert!(sibling_test_candidates("pkg/foo.go").contains(&"pkg/foo_test.go".to_string()));
        assert_eq!(sibling_test_candidates("src/foo.rs"), vec!["src/foo.rs"]);
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
