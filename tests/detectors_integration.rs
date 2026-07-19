use codasaurus::config::Config;
use codasaurus::detectors;
use codasaurus::parser::{self, ParsedFile};

/// Helper to build a ParsedFile from raw content
fn make_file(path: &str, _language: &str, content: &str) -> ParsedFile {
    parser::parse_file(path, content).unwrap()
}

/// Helper to build a Config with all checks enabled
fn all_checks_config() -> Config {
    Config {
        checks: codasaurus::config::CheckConfig {
            hallucinated_imports: true,
            phantom_deps: true,
            vulnerabilities: true,
            secrets: true,
            over_engineering: true,
            boilerplate: true,
            stale_api: true,
            todo_leaks: true,
            guidelines: true,
            exclude_patterns: vec![],
        },
        ..Config::default()
    }
}

// ---------------------------------------------------------------------------
// Integration: Clean code produces no findings
// ---------------------------------------------------------------------------

#[test]
fn test_clean_rust_file_no_findings() {
    let content = r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {
    let _result = add(1, 2);
}
"#;
    let file = make_file("lib.rs", "rust", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    assert!(
        findings.is_empty(),
        "clean Rust should have no findings, got: {:?}",
        findings.findings
    );
}

#[test]
fn test_clean_python_file_no_findings() {
    let content = r#"
def add(a: int, b: int) -> int:
    return a + b

def main() -> None:
    result = add(1, 2)
    print(result)
"#;
    let file = make_file("main.py", "python", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    assert!(
        findings.is_empty(),
        "clean Python should have no findings, got: {:?}",
        findings.findings
    );
}

#[test]
fn test_clean_js_file_no_findings() {
    let content = r#"
function add(a, b) {
    return a + b;
}
"#;
    let file = make_file("util.js", "javascript", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    assert!(
        findings.is_empty(),
        "clean JS should have no findings, got: {:?}",
        findings.findings
    );
}

// ---------------------------------------------------------------------------
// Integration: Security detector (secrets + todo-leaks)
// ---------------------------------------------------------------------------

#[test]
fn test_detects_api_key_leak() {
    let content = r#"
const API_KEY = "sk-1234567890abcdef1234567890abcdef";
console.log(API_KEY);
"#;
    let file = make_file("config.js", "javascript", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    let secret_findings: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "secrets")
        .collect();
    assert!(!secret_findings.is_empty(), "should detect API key leak");
}

#[test]
fn test_detects_todo_placeholder() {
    let content = r#"
// TODO: implement error handling
function process() {
    // FIXME: this is slow
    return null;
}
"#;
    let file = make_file("app.js", "javascript", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    let todo_findings: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "todo-leaks")
        .collect();
    assert!(
        !todo_findings.is_empty(),
        "should detect TODO/FIXME placeholders"
    );
}

#[test]
fn test_detects_aws_key() {
    let content = r#"
AWS_ACCESS_KEY_ID = "AKIAIOSFODNN7EXAMPLE";
"#;
    let file = make_file("config.py", "python", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    let secret_findings: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "secrets")
        .collect();
    assert!(!secret_findings.is_empty(), "should detect AWS access key");
}

// ---------------------------------------------------------------------------
// Integration: Stale API detection
// ---------------------------------------------------------------------------

#[test]
fn test_detects_stale_react_api() {
    let content = r#"
class MyComponent extends React.Component {
    componentWillMount() {
        // old lifecycle
    }
}
"#;
    let file = make_file("component.jsx", "jsx", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    let stale_findings: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "stale-api")
        .collect();
    assert!(
        !stale_findings.is_empty(),
        "should detect deprecated componentWillMount"
    );
}

#[test]
fn test_detects_stale_rust_try_macro() {
    let content = r#"
fn read_file() -> Result<String, io::Error> {
    let content = try!(fs::read_to_string("foo.txt"));
    Ok(content)
}
"#;
    let file = make_file("reader.rs", "rust", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    let stale_findings: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "stale-api")
        .collect();
    assert!(!stale_findings.is_empty(), "should detect try! macro");
}

#[test]
fn test_detects_stale_urllib2() {
    let content = "import urllib2\n";
    let file = make_file("http.py", "python", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    let stale_findings: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "stale-api")
        .collect();
    assert!(!stale_findings.is_empty(), "should detect urllib2 import");
}

// ---------------------------------------------------------------------------
// Integration: Over-engineering detection
// ---------------------------------------------------------------------------

#[test]
fn test_detects_single_impl_trait() {
    let content = r#"
pub trait MyService {
    fn execute(&self) -> i32;
}

struct ServiceImpl;

impl MyService for ServiceImpl {
    fn execute(&self) -> i32 { 42 }
}
"#;
    let file = make_file("service.rs", "rust", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    let style_findings: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "over-engineering")
        .collect();
    assert!(
        !style_findings.is_empty(),
        "should detect single-impl trait as over-engineering"
    );
}

#[test]
fn test_detects_unnecessary_factory() {
    let content = r#"
class UserFactory {
    create() { return new User(); }
}

class User {
    constructor() { this.name = ""; }
}
"#;
    let file = make_file("factory.js", "javascript", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    let style_findings: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "over-engineering")
        .collect();
    assert!(
        !style_findings.is_empty(),
        "should detect unnecessary factory pattern"
    );
}

// ---------------------------------------------------------------------------
// Integration: Boilerplate detection
// ---------------------------------------------------------------------------

#[test]
fn test_detects_long_function() {
    let mut lines = vec!["fn process() {".to_string()];
    for i in 0..65 {
        lines.push(format!("    let x_{i} = {i};"));
    }
    lines.push("}".to_string());
    let content = lines.join("\n");
    let file = make_file("processor.rs", "rust", &content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    let boilerplate: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "boilerplate")
        .collect();
    assert!(
        !boilerplate.is_empty(),
        "should detect long function as boilerplate"
    );
}

// ---------------------------------------------------------------------------
// Integration: Phantom deps (local-only, no network)
// ---------------------------------------------------------------------------

#[test]
fn test_phantom_deps_missing_from_package_json() {
    let dep_file = make_file(
        "package.json",
        "javascript",
        r#"{
    "dependencies": {
        "express": "^4.0.0"
    }
}"#,
    );
    let source_file = make_file(
        "app.js",
        "javascript",
        "import { Router } from 'express';\nimport { createClient } from 'ioredis';\n",
    );
    let findings = detectors::run_all(&[dep_file, source_file], &all_checks_config());
    let phantom: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "phantom-deps")
        .collect();
    assert!(!phantom.is_empty(), "should detect ioredis as phantom dep");
    assert!(
        phantom.iter().any(|f| f.message.contains("ioredis")),
        "phantom dep finding should mention ioredis"
    );
}

#[test]
fn test_phantom_deps_rust_missing_from_cargo_toml() {
    let dep_file = make_file(
        "Cargo.toml",
        "rust",
        r#"[dependencies]
serde = "1"
"#,
    );
    let source_file = make_file(
        "src/main.rs",
        "rust",
        "use serde::Serialize;\nuse tokio::runtime::Runtime;\n",
    );
    let findings = detectors::run_all(&[dep_file, source_file], &all_checks_config());
    let phantom: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "phantom-deps")
        .collect();
    assert!(!phantom.is_empty(), "should detect tokio as phantom dep");
}

#[test]
fn test_no_phantom_dep_when_declared() {
    let dep_file = make_file(
        "package.json",
        "javascript",
        r#"{
    "dependencies": {
        "express": "^4.0.0",
        "axios": "^1.0.0"
    }
}"#,
    );
    let source_file = make_file(
        "app.js",
        "javascript",
        "const express = require('express');\nconst axios = require('axios');\n",
    );
    let findings = detectors::run_all(&[dep_file, source_file], &all_checks_config());
    let phantom: Vec<_> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "phantom-deps")
        .collect();
    assert!(
        phantom.is_empty(),
        "all deps declared, no phantom deps expected"
    );
}

// ---------------------------------------------------------------------------
// Integration: Severity counting
// ---------------------------------------------------------------------------

#[test]
fn test_severity_counting() {
    let content = r#"
// TODO: finish this
const AWS_KEY = "AKIAIOSFODNN7EXAMPLE";
class UserFactory {
    create() { return new User(); }
}
class User {
    constructor() { this.name = ""; this.email = ""; }
}
"#;
    let file = make_file("messy.js", "javascript", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    let by_severity = findings.count_by_severity();
    // Secrets are "blocking", TODO leaks are "warning", stale-api/over-engineering
    // may add "info"/"warning" findings
    assert!(
        by_severity.contains_key("blocking") || by_severity.contains_key("warning"),
        "should have at least blocking or warning findings, got: {by_severity:?}"
    );
}

// ---------------------------------------------------------------------------
// Integration: Mixed detector output
// ---------------------------------------------------------------------------

#[test]
fn test_mixed_findings_from_multiple_detectors() {
    // One file that triggers stale-api, secrets, todo-leaks, and over-engineering
    let content = r#"
// TODO: clean this up later
const AWS_KEY = "AKIAIOSFODNN7EXAMPLE";

class UserFactory {
    createUser() { return new User(); }
}

class User {
    constructor() { this.name = ""; }
}

class MyComponent {
    componentWillMount() {}
}
"#;
    let file = make_file("messy.jsx", "jsx", content);
    let findings = detectors::run_all(&[file], &all_checks_config());
    let detectors_triggered: std::collections::HashSet<&str> = findings
        .findings
        .iter()
        .map(|f| f.detector.as_str())
        .collect();
    assert!(
        detectors_triggered.contains("secrets"),
        "should trigger secrets detector"
    );
    assert!(
        detectors_triggered.contains("todo-leaks"),
        "should trigger todo-leaks detector"
    );
}

// ---------------------------------------------------------------------------
// Integration: Finding structure validation
// ---------------------------------------------------------------------------

#[test]
fn test_finding_has_required_fields() {
    let content = "const AWS_KEY = \"AKIAIOSFODNN7EXAMPLE\";\n";
    let file = make_file("leak.js", "javascript", content);
    let findings = detectors::run_all(&[file], &all_checks_config());

    for finding in &findings.findings {
        assert!(
            !finding.detector.is_empty(),
            "detector name must not be empty"
        );
        assert!(!finding.severity.is_empty(), "severity must not be empty");
        assert!(!finding.file.is_empty(), "file path must not be empty");
        assert!(!finding.message.is_empty(), "message must not be empty");
        assert!(
            finding.severity == "blocking"
                || finding.severity == "warning"
                || finding.severity == "info",
            "severity must be valid, got: {}",
            finding.severity
        );
    }
}

// ---------------------------------------------------------------------------
// Integration: Multiple clean files across languages
// ---------------------------------------------------------------------------

#[test]
fn test_multi_language_clean_files() {
    let rs = make_file(
        "math.rs",
        "rust",
        "pub fn add(x: i32, y: i32) -> i32 { x + y }",
    );
    let py = make_file(
        "math.py",
        "python",
        "def add(x: int, y: int) -> int:\n    return x + y",
    );
    let js = make_file(
        "math.js",
        "javascript",
        "function add(x, y) { return x + y; }",
    );
    let ts = make_file(
        "math.ts",
        "typescript",
        "export function add(x: number, y: number): number { return x + y; }",
    );
    let go = make_file(
        "math.go",
        "go",
        "package main\nfunc add(x int, y int) int { return x + y }",
    );
    let java = make_file(
        "Math.java",
        "java",
        "class Math { static int add(int x, int y) { return x + y; } }",
    );

    let findings = detectors::run_all(&[rs, py, js, ts, go, java], &all_checks_config());
    assert!(
        findings.is_empty(),
        "clean files in 6 languages should have no findings, got: {:?}",
        findings.findings
    );
}
