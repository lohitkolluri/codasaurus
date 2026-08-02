use crate::detectors::Finding;
use crate::parser::ParsedFile;
use crate::registry;
use std::collections::HashSet;

static COPYLEFT_KEYWORDS: &[&str] = &[
    "GPL",
    "GPL-2",
    "GPL-3",
    "AGPL",
    "LGPL",
    "MPL",
    "EUPL",
    "CC-BY-NC",
    "Proprietary",
    "GNU",
];

pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut checked = HashSet::new();

    for file in parsed_files {
        let Some((registry, deps)) = registry_for(file) else {
            continue;
        };
        for dep in deps {
            if !checked.insert((registry.to_string(), dep.clone())) {
                continue;
            }
            check_dep(&mut findings, file, registry, &dep);
        }
    }

    findings
}

fn registry_for(file: &ParsedFile) -> Option<(&'static str, Vec<String>)> {
    let path = file.path.to_lowercase();
    if path.ends_with("package.json") {
        Some((
            "npm",
            crate::dep_parser::extract_npm_deps(&file.raw_content),
        ))
    } else if path.ends_with("requirements.txt")
        || path.ends_with("setup.py")
        || path.ends_with("setup.cfg")
    {
        Some((
            "pypi",
            crate::dep_parser::extract_requirements_deps(&file.raw_content),
        ))
    } else if path.ends_with("pyproject.toml") {
        Some((
            "pypi",
            crate::dep_parser::extract_pyproject_deps(&file.raw_content),
        ))
    } else if path.ends_with("cargo.toml") {
        Some((
            "crates.io",
            crate::dep_parser::extract_cargo_deps(&file.raw_content),
        ))
    } else {
        None
    }
}

fn check_dep(findings: &mut Vec<Finding>, file: &ParsedFile, registry: &str, dep: &str) {
    match registry::get_metadata(registry, dep) {
        Ok(Some(meta)) => {
            if let Some(license) = meta.license {
                if is_copyleft(&license) {
                    findings.push(Finding {
                        detector: "license-drift".to_string(),
                        severity: "warning",
                        file: file.path.clone(),
                        line: 0,
                        column: 0,
                        message: format!(
                            "Dependency `{dep}` declares a copyleft-style license (`{license}`). Review before merging."
                        ),
                        suggestion: Some(
                            "Verify the license is compatible with your project policy.".to_string()
                        ),
                        evidence: Some(license),
                        codemod: None,
                        confidence: None,
                        judge_rationale: None,
                    });
                }
            }
        }
        Ok(None) => {}
        Err(e) => {
            tracing::debug!(error = %e, "license metadata lookup failed");
        }
    }
}

fn is_copyleft(license: &str) -> bool {
    let upper = license.to_ascii_uppercase();
    COPYLEFT_KEYWORDS.iter().any(|kw| upper.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copyleft_detection() {
        assert!(is_copyleft("GPL-3.0"));
        assert!(is_copyleft("GNU General Public License v2.0"));
        assert!(is_copyleft("AGPL-3.0"));
        assert!(!is_copyleft("MIT"));
        assert!(!is_copyleft("Apache-2.0"));
    }
}
