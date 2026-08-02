use crate::detectors::Finding;
use crate::parser::ParsedFile;

/// Detect deprecated methods and known API migrations
pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for file in parsed_files {
        for line in &file.lines {
            for pattern in STALE_PATTERNS.iter() {
                if pattern.regex.is_match(&line.content) {
                    findings.push(Finding {
                        detector: "stale-api".to_string(),
                        severity: pattern.severity,
                        file: file.path.clone(),
                        line: line.number,
                        column: 0,
                        message: pattern.message.to_string(),
                        suggestion: Some(pattern.suggestion.to_string()),
                        evidence: Some(line.content.clone()),
                        codemod: pattern.codemod.map(|s| s.to_string()),
                        confidence: None,
                        judge_rationale: None,
                        reachability: None,
                    });
                }
            }
        }
    }

    findings
}

struct StalePattern {
    _name: &'static str,
    severity: &'static str,
    regex: regex::Regex,
    message: &'static str,
    suggestion: &'static str,
    codemod: Option<&'static str>,
}

impl StalePattern {
    fn new(
        name: &'static str,
        severity: &'static str,
        pattern: &str,
        message: &'static str,
        suggestion: &'static str,
    ) -> Self {
        Self {
            _name: name,
            severity,
            regex: regex::Regex::new(pattern).expect("invalid stale-api regex"),
            message,
            suggestion,
            codemod: None,
        }
    }

    fn with_codemod(
        name: &'static str,
        severity: &'static str,
        pattern: &str,
        message: &'static str,
        suggestion: &'static str,
        codemod: &'static str,
    ) -> Self {
        Self {
            _name: name,
            severity,
            regex: regex::Regex::new(pattern).expect("invalid stale-api regex"),
            message,
            suggestion,
            codemod: Some(codemod),
        }
    }
}

use std::sync::LazyLock;

static STALE_PATTERNS: LazyLock<Vec<StalePattern>> = LazyLock::new(|| {
    vec![
        // --- JavaScript / TypeScript ---
        StalePattern::new(
            "react-componentWillMount",
            "info",
            r"componentWillMount\s*\(",
            "`componentWillMount` is deprecated. Use `componentDidMount` or constructor instead.",
            "Replace `componentWillMount` with `componentDidMount` or move logic to the constructor.",
        ),
        StalePattern::new(
            "react-componentWillUpdate",
            "info",
            r"componentWillUpdate\s*\(",
            "`componentWillUpdate` is deprecated. Use `componentDidUpdate` or `getSnapshotBeforeUpdate` instead.",
            "Replace `componentWillUpdate` with `componentDidUpdate`.",
        ),
        StalePattern::new(
            "react-componentWillReceiveProps",
            "info",
            r"componentWillReceiveProps\s*\(",
            "`componentWillReceiveProps` is deprecated. Use `getDerivedStateFromProps` instead.",
            "Replace `componentWillReceiveProps` with `getDerivedStateFromProps`.",
        ),
        StalePattern::new(
            "react-findDOMNode",
            "warning",
            r"findDOMNode\s*\(",
            "`findDOMNode` is deprecated. Use refs with callback or createRef instead.",
            "Replace `findDOMNode` with a ref callback or `createRef`.",
        ),
        StalePattern::new(
            "react-string-refs",
            "warning",
            r#"this\.refs\.\w+"#,
            "String refs are deprecated. Use `React.createRef()` or callback refs instead.",
            "Replace string refs with `React.createRef()` or `useRef()`.",
        ),
        StalePattern::new(
            "react-propTypes-deprecated",
            "warning",
            r#"import\s+PropTypes\s+from\s+['"]prop-types['"]"#,
            "Using PropTypes when TypeScript types are preferred. Consider using TypeScript interfaces instead.",
            "Replace PropTypes with TypeScript interfaces or types.",
        ),
        // --- Python ---
        StalePattern::new(
            "python-urllib2",
            "warning",
            r"import urllib2",
            "`urllib2` was merged into `urllib.request` in Python 3. Use `urllib.request` instead.",
            "Replace `import urllib2` with `import urllib.request`.",
        ),
        StalePattern::new(
            "python-print-statement",
            "info",
            r#"(?m)^\s*print\s+"[^"]*"$"#,
            "Python 2 print statement detected. Use `print()` function instead.",
            "Replace `print \"...\"` with `print(\"...\")`.",
        ),
        StalePattern::new(
            "python-string-exceptions",
            "warning",
            r#"raise\s+['"][^'"]*['"]"#,
            "Raising string exceptions is deprecated. Use an Exception subclass.",
            "Replace the string with an appropriate Exception class (e.g. `ValueError`, `TypeError`).",
        ),
        // --- Rust ---
        StalePattern::new(
            "rust-try-macro",
            "info",
            r"try!",
            "The `try!` macro is deprecated. Use the `?` operator instead.",
            "Replace `try!` with `?`.",
        ),
        StalePattern::new(
            "rust-extern-crate",
            "info",
            r"extern crate \w+;",
            "`extern crate` is no longer needed in Rust 2018+. Use `use` directly.",
            "Remove `extern crate` and add the crate to Cargo.toml if needed.",
        ),
        // --- CI / Docker ---
        StalePattern::with_codemod(
            "docker-add-curl",
            "info",
            r"apt-get install.* curl",
            "`curl` is available by default in most modern images. Avoid re-installing.",
            "Remove `curl` from the install list if the base image already includes it.",
            "apt-get install -y --no-install-recommends ",
        ),
        // --- Node.js ---
        StalePattern::new(
            "node-deprecated-api",
            "warning",
            r#"require\s*\(\s*['"]request['"]\s*\)"#,
            "The `request` package is deprecated. Use `fetch` (built-in) or `got`/`undici`.",
            "Replace `require('request')` with the built-in `fetch` or `undici`.",
        ),
        StalePattern::new(
            "node-util-promisify-deprecated",
            "info",
            r"util\.promisify",
            "`util.promisify` is deprecated in favor of `fs.promises` and native Promise APIs.",
            "Use native Promise-based APIs instead of `util.promisify`.",
        ),
        // --- Modern framework / API drift (replaces common LLM nits) ---
        StalePattern::new(
            "react-createClass",
            "warning",
            r"React\.createClass\s*\(",
            "`React.createClass` is removed from modern React. Use functions or ES6 classes.",
            "Convert to a function component or `class extends React.Component`.",
        ),
        StalePattern::new(
            "jquery-ajax",
            "info",
            r"\$\.ajax\s*\(",
            "`$.ajax` is legacy; prefer `fetch` / native HTTP clients.",
            "Replace jQuery AJAX with `fetch` or your app's HTTP helper.",
        ),
        StalePattern::new(
            "moment-js",
            "info",
            r#"from\s+['"]moment['"]|require\s*\(\s*['"]moment['"]\s*\)"#,
            "`moment` is in maintenance mode. Prefer `Temporal`, `dayjs`, or `date-fns`.",
            "Migrate away from Moment.js to a lighter date library.",
        ),
        StalePattern::new(
            "enzyme",
            "info",
            r#"from\s+['"]enzyme['"]|require\s*\(\s*['"]enzyme['"]\s*\)"#,
            "Enzyme is unmaintained for modern React. Prefer Testing Library / Playwright.",
            "Replace Enzyme with `@testing-library/react`.",
        ),
        StalePattern::new(
            "python-asyncio-get-event-loop",
            "info",
            r"asyncio\.get_event_loop\s*\(\s*\)",
            "`asyncio.get_event_loop()` is discouraged; use `get_running_loop()` / `asyncio.run()`.",
            "Prefer `asyncio.run()` or `asyncio.get_running_loop()`.",
        ),
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_stale_react_api() {
        let file = ParsedFile {
            path: "test.js".to_string(),
            language: "javascript".to_string(),
            raw_content: "componentWillMount()".to_string(),
            lines: vec![crate::parser::SourceLine {
                number: 1,
                content: "componentWillMount()".to_string(),
            }],
            imports: vec![],
        };
        let findings = detect(&[file]);
        assert!(
            !findings.is_empty(),
            "should detect stale componentWillMount"
        );
        assert_eq!(findings[0].detector, "stale-api");
    }

    #[test]
    fn test_detect_stale_python_api() {
        let file = ParsedFile {
            path: "test.py".to_string(),
            language: "python".to_string(),
            raw_content: "import urllib2".to_string(),
            lines: vec![crate::parser::SourceLine {
                number: 1,
                content: "import urllib2".to_string(),
            }],
            imports: vec![],
        };
        let findings = detect(&[file]);
        assert!(!findings.is_empty(), "should detect stale urllib2 import");
    }

    #[test]
    fn test_detect_stale_rust_api() {
        let file = ParsedFile {
            path: "test.rs".to_string(),
            language: "rust".to_string(),
            raw_content: "try!(foo())".to_string(),
            lines: vec![crate::parser::SourceLine {
                number: 1,
                content: "try!(foo())".to_string(),
            }],
            imports: vec![],
        };
        let findings = detect(&[file]);
        assert!(!findings.is_empty(), "should detect stale try! macro");
    }

    #[test]
    fn test_clean_code_no_findings() {
        let file = ParsedFile {
            path: "test.js".to_string(),
            language: "javascript".to_string(),
            raw_content: "const x = 1;\nfunction foo() { return x; }".to_string(),
            lines: vec![
                crate::parser::SourceLine {
                    number: 1,
                    content: "const x = 1;".to_string(),
                },
                crate::parser::SourceLine {
                    number: 2,
                    content: "function foo() { return x; }".to_string(),
                },
            ],
            imports: vec![],
        };
        let findings = detect(&[file]);
        assert!(
            findings.is_empty(),
            "clean code should have no stale-api findings"
        );
    }
}
