//! Golden detector regression fixtures — catch "still find X / don't spam Y".
//!
//! Fixtures live under `tests/fixtures/golden/<case>/`:
//! - `input.rs` (or `.ts` / `.py` / `.tf`) — source under review
//! - `expect.json` — `{ "must_include": ["secrets"], "must_exclude": ["boilerplate"], "max_findings": 5 }`

use codasaurus::config::Config;
use codasaurus::detectors;
use codasaurus::parser;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct Expect {
    #[serde(default)]
    must_include: Vec<String>,
    #[serde(default)]
    must_exclude: Vec<String>,
    #[serde(default)]
    max_findings: Option<usize>,
}

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
            risky_patterns: true,
            todo_leaks: true,
            guidelines: true,
            graph: true,
            iac: true,
            exclude_patterns: vec![],
        },
        ..Config::default()
    }
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden")
}

fn run_case(name: &str) {
    let dir = fixture_root().join(name);
    let expect: Expect = serde_json::from_str(
        &fs::read_to_string(dir.join("expect.json")).unwrap_or_else(|e| {
            panic!("missing expect.json for {name}: {e}");
        }),
    )
    .expect("valid expect.json");

    let input = ["input.rs", "input.ts", "input.py", "input.tf", "input.js"]
        .iter()
        .find_map(|f| {
            let p = dir.join(f);
            fs::read_to_string(&p).ok().map(|c| (f.to_string(), c))
        })
        .unwrap_or_else(|| panic!("no input.* for golden case {name}"));

    let path = format!("golden/{name}/{}", input.0);
    let parsed = parser::parse_file(&path, &input.1).expect("parse fixture");
    let findings = detectors::run_all(&[parsed], &all_checks_config(), None);
    let detectors_hit: Vec<&str> = findings
        .findings
        .iter()
        .map(|f| f.detector.as_str())
        .collect();

    for need in &expect.must_include {
        assert!(
            detectors_hit.iter().any(|d| *d == need || d.contains(need)),
            "case {name}: expected detector `{need}`, got {detectors_hit:?}"
        );
    }
    for ban in &expect.must_exclude {
        assert!(
            !detectors_hit.iter().any(|d| *d == ban || d.contains(ban)),
            "case {name}: unexpected detector `{ban}` in {detectors_hit:?}"
        );
    }
    if let Some(max) = expect.max_findings {
        assert!(
            findings.findings.len() <= max,
            "case {name}: too many findings {} > {max}",
            findings.findings.len()
        );
    }
}

#[test]
fn golden_secret_leak() {
    run_case("secret_leak");
}

#[test]
fn golden_clean_helper() {
    run_case("clean_helper");
}

#[test]
fn golden_todo_placeholder() {
    run_case("todo_placeholder");
}
