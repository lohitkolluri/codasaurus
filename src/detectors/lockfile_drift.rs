use crate::detectors::Finding;
use crate::parser::ParsedFile;
use std::collections::{HashMap, HashSet};

pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let manifests = collect_manifests(parsed_files);

    for (dir, manifest) in manifests {
        let spec = manifest.kind.lockfile();
        if let Some(lockfile) = find_lockfile(parsed_files, &dir, spec.name) {
            let locked = (spec.extract)(&lockfile.raw_content);
            for dep in &manifest.deps {
                if !locked.contains(dep) {
                    findings.push(lockfile_finding(&manifest.path, dep, spec.name));
                }
            }
        }
    }

    findings
}

enum ManifestKind {
    Npm,
    Cargo,
    Go,
}

type ManifestExtractor = fn(&str) -> Vec<String>;
type LockfileExtractor = fn(&str) -> HashSet<String>;

struct LockfileSpec {
    name: &'static str,
    extract: LockfileExtractor,
}

impl ManifestKind {
    fn lockfile(self) -> LockfileSpec {
        let (name, extract): (&'static str, LockfileExtractor) = match self {
            ManifestKind::Npm => ("package-lock.json", extract_npm_lock_deps),
            ManifestKind::Cargo => ("Cargo.lock", extract_cargo_lock_deps),
            ManifestKind::Go => ("go.sum", extract_go_sum_deps),
        };
        LockfileSpec { name, extract }
    }
}

struct Manifest {
    path: String,
    deps: HashSet<String>,
    kind: ManifestKind,
}

fn collect_manifests(parsed_files: &[ParsedFile]) -> HashMap<String, Manifest> {
    let mut manifests = HashMap::new();
    for file in parsed_files {
        let path = file.path.to_lowercase();
        let Some((kind, extract)) = manifest_kind_for(&path) else {
            continue;
        };
        let deps = extract(&file.raw_content);
        if deps.is_empty() {
            continue;
        }
        manifests.insert(
            directory_of(&file.path),
            Manifest {
                path: file.path.clone(),
                deps: deps.into_iter().collect(),
                kind,
            },
        );
    }
    manifests
}

fn manifest_kind_for(path: &str) -> Option<(ManifestKind, ManifestExtractor)> {
    if path.ends_with("package.json") {
        Some((ManifestKind::Npm, crate::dep_parser::extract_npm_deps))
    } else if path.ends_with("cargo.toml") {
        Some((ManifestKind::Cargo, crate::dep_parser::extract_cargo_deps))
    } else if path.ends_with("go.mod") {
        Some((ManifestKind::Go, crate::dep_parser::extract_go_mod_deps))
    } else {
        None
    }
}

fn directory_of(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    }
}

fn find_lockfile<'a>(
    parsed_files: &'a [ParsedFile],
    dir: &str,
    filename: &str,
) -> Option<&'a ParsedFile> {
    parsed_files.iter().find(|f| {
        let fdir = directory_of(&f.path);
        fdir == dir && f.path.rsplit('/').next().unwrap_or("") == filename
    })
}

fn extract_npm_lock_deps(content: &str) -> HashSet<String> {
    let mut deps = HashSet::new();
    let Ok(json) = serde_json::from_str::<serde_json::Value>(content) else {
        return deps;
    };
    // package-lock v2/v3 uses packages["node_modules/<name>"].
    if let Some(packages) = json.get("packages").and_then(|p| p.as_object()) {
        for key in packages.keys() {
            if let Some(name) = key.strip_prefix("node_modules/") {
                deps.insert(name.to_string());
            }
        }
    }
    // package-lock v1 uses dependencies { name: { version } }.
    if let Some(deps_obj) = json.get("dependencies").and_then(|d| d.as_object()) {
        for key in deps_obj.keys() {
            deps.insert(key.to_string());
        }
    }
    deps
}

fn extract_cargo_lock_deps(content: &str) -> HashSet<String> {
    let mut deps = HashSet::new();
    let Ok(table) = content.parse::<toml::Table>() else {
        return deps;
    };
    if let Some(packages) = table.get("package").and_then(|p| p.as_array()) {
        for pkg in packages {
            if let Some(name) = pkg.get("name").and_then(|n| n.as_str()) {
                deps.insert(name.to_string());
            }
        }
    }
    deps
}

fn extract_go_sum_deps(content: &str) -> HashSet<String> {
    let mut deps = HashSet::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Lines look like: "github.com/foo/bar v1.2.3 h1:..."
        if let Some((module, rest)) = line.split_once(' ') {
            if let Some(version) = rest.split_whitespace().next() {
                if version.starts_with('v') {
                    deps.insert(module.to_string());
                }
            }
        }
    }
    deps
}

fn lockfile_finding(manifest_path: &str, dep: &str, lockfile: &str) -> Finding {
    Finding {
        detector: "lockfile-drift".to_string(),
        severity: "warning",
        file: manifest_path.to_string(),
        line: 0,
        column: 0,
        message: format!(
            "Dependency `{dep}` is declared in {manifest_path} but is missing from {lockfile}. The lockfile is stale."
        ),
        suggestion: Some(
            "Run the lockfile update command for your package manager (e.g. `npm install`, `cargo update`, `go mod tidy`) and commit the result.".to_string(),
        ),
        evidence: Some(dep.to_string()),
        codemod: None,
        confidence: None,
        judge_rationale: None,
            reachability: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    fn parse(name: &str, content: &str) -> ParsedFile {
        parse_file(name, content).unwrap()
    }

    #[test]
    fn npm_drift_missing_dep() {
        let files = [
            parse("package.json", r#"{"dependencies": {"react": "^18.0.0"}}"#),
            parse(
                "package-lock.json",
                r#"{"packages": {"node_modules/lodash": {"version": "4.0.0"}}}"#,
            ),
        ];
        let findings = detect(&files);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("react"));
    }

    #[test]
    fn cargo_drift_missing_dep() {
        let files = [
            parse("Cargo.toml", "[dependencies]\nserde = \"1.0\"\n"),
            parse(
                "Cargo.lock",
                "[[package]]\nname = \"tokio\"\nversion = \"1.0.0\"\n",
            ),
        ];
        let findings = detect(&files);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("serde"));
    }

    #[test]
    fn go_drift_missing_dep() {
        let files = [
            parse(
                "go.mod",
                "module github.com/example\n\ngo 1.21\n\nrequire github.com/gorilla/mux v1.8.0\n",
            ),
            parse("go.sum", "github.com/sirupsen/logrus v1.9.0 h1:abc\n"),
        ];
        let findings = detect(&files);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("gorilla/mux"));
    }
}
