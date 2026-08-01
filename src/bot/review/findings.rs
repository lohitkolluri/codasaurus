#[cfg(test)]
use crate::detectors::Finding;

pub(crate) fn severity_at_least(sev: &str, min: &str) -> bool {
    fn rank(s: &str) -> u8 {
        match s {
            "blocking" => 3,
            "warning" => 2,
            _ => 1,
        }
    }
    rank(sev) >= rank(min)
}

#[cfg(test)]
fn build_comment_body(finding: &Finding) -> String {
    crate::bot::markdown::inline_finding_comment(finding)
}

pub(crate) fn collect_registry_pairs(files: &[crate::parser::ParsedFile]) -> Vec<(String, String)> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for file in files {
        let registry = match file.language.as_str() {
            "javascript" | "typescript" | "tsx" | "jsx" => "npm",
            "python" => "pypi",
            "rust" => "crates.io",
            _ => continue,
        };
        for import in &file.imports {
            let Some(package) = crate::detectors::extract_package_name(&import.name) else {
                continue;
            };
            if package.starts_with('.') || package.starts_with('/') {
                continue;
            }
            if crate::detectors::hallucinated_imports::is_builtin(&package, registry) {
                continue;
            }
            let key = (registry.to_string(), package);
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
    }
    out
}

pub(crate) fn merge_vulnerability_findings(
    findings: &[crate::detectors::Finding],
) -> Vec<crate::detectors::Finding> {
    use crate::detectors::Finding;
    use std::collections::BTreeMap;

    let mut groups: BTreeMap<(String, usize), Vec<&Finding>> = BTreeMap::new();
    let mut non_vuln: Vec<&Finding> = Vec::new();

    for f in findings {
        if f.detector == "vulnerabilities" && f.line > 0 {
            groups.entry((f.file.clone(), f.line)).or_default().push(f);
        } else {
            non_vuln.push(f);
        }
    }

    let mut result: Vec<Finding> = non_vuln.into_iter().cloned().collect();

    for ((file, line), group) in groups {
        if group.len() <= 1 {
            result.extend(group.into_iter().cloned());
            continue;
        }
        let max_sev: &str = group
            .iter()
            .map(|f| f.severity)
            .max_by_key(|s| match *s {
                "blocking" => 3,
                "warning" => 2,
                _ => 1,
            })
            .unwrap_or("info");
        let cve_list: Vec<&str> = group
            .iter()
            .filter_map(|f| f.message.split(':').next())
            .collect();
        let count = group.len();
        result.push(Finding {
            file,
            line,
            column: 0,
            severity: match max_sev {
                "blocking" => "blocking",
                "warning" => "warning",
                _ => "info",
            },
            detector: "vulnerabilities".into(),
            message: format!(
                "{} known CVE{}: {}",
                count,
                if count == 1 { "" } else { "s" },
                cve_list.join(", ")
            ),
            suggestion: group.first().and_then(|f| f.suggestion.clone()),
            codemod: None,
            evidence: None,
        });
    }
    result
}

pub(crate) fn is_critical_full_file_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("auth")
        || lower.contains("crypto")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("middleware")
        || lower.contains("rbac")
        || lower.contains("permission")
        || lower.contains(".github/workflows")
        || lower.ends_with("dockerfile")
        || lower.contains("dockerfile.")
        || lower.ends_with(".tf")
        || lower.ends_with(".tfvars")
        || lower.contains("/k8s/")
        || lower.contains("/kubernetes/")
        || lower.contains("/helm/")
        || lower.contains("deployment.yaml")
        || lower.contains("deployment.yml")
        || lower.contains("serviceaccount")
        || lower.ends_with("compose.yml")
        || lower.ends_with("compose.yaml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::Finding;

    fn f(
        detector: &str,
        severity: &'static str,
        file: &str,
        line: usize,
        msg: &str,
        sug: Option<&str>,
    ) -> Finding {
        Finding {
            detector: detector.into(),
            severity,
            file: file.into(),
            line,
            column: 0,
            message: msg.into(),
            suggestion: sug.map(|s| s.into()),
            evidence: None,
            codemod: None,
        }
    }

    #[test]
    fn comment_body_hallucinated_import() {
        let body = build_comment_body(&f(
            "hallucinated-imports",
            "blocking",
            "src/a.ts",
            5,
            "Package `fakelib` not found on npm.",
            Some("Check npmjs.com"),
        ));
        assert!(body.contains("Package does not exist"));
        assert!(body.contains("fakelib"));
        assert!(body.contains("fingerprint:"));
        assert!(body.contains("`blocking`"));
    }

    #[test]
    fn comment_body_secret() {
        let body = build_comment_body(&f(
            "secrets",
            "blocking",
            "src/x.ts",
            10,
            "API Key detected",
            Some("Use env vars"),
        ));
        assert!(body.contains("Credential in source"));
        assert!(body.contains("secret") || body.contains("Rotate"));
    }

    #[test]
    fn comment_body_vulnerability() {
        let body = build_comment_body(&f(
            "vulnerabilities",
            "warning",
            "pkg.json",
            7,
            "GHSA-123: desc",
            Some("Update `lodash`"),
        ));
        assert!(body.contains("Known vulnerability"));
        assert!(body.contains("lodash"));
    }

    #[test]
    fn comment_body_todo() {
        let body = build_comment_body(&f(
            "todo-leaks",
            "warning",
            "src/a.ts",
            15,
            "// TODO: fix",
            Some("Complete it"),
        ));
        assert!(body.contains("Incomplete code"));
    }

    #[test]
    fn merge_vulns_collapses_same_line() {
        let findings = vec![
            f(
                "vulnerabilities",
                "warning",
                "x.json",
                1,
                "CVE-1: d1",
                Some("Up `lodash`"),
            ),
            f(
                "vulnerabilities",
                "warning",
                "x.json",
                1,
                "CVE-2: d2",
                Some("Up `lodash`"),
            ),
            f(
                "hallucinated-imports",
                "blocking",
                "a.ts",
                5,
                "not found",
                Some("Check npm"),
            ),
        ];
        let merged = merge_vulnerability_findings(&findings);
        assert_eq!(merged.len(), 2); // 2 vulns merged + 1 non-vuln
    }

    #[test]
    fn merge_vulns_keeps_single() {
        let findings = vec![
            f(
                "vulnerabilities",
                "warning",
                "x.json",
                1,
                "CVE-1: d1",
                Some("Up `lodash`"),
            ),
            f(
                "vulnerabilities",
                "blocking",
                "y.json",
                2,
                "CVE-2: d2",
                Some("Up `zod`"),
            ),
        ];
        let merged = merge_vulnerability_findings(&findings);
        assert_eq!(merged.len(), 2); // different files, NOT merged
    }

    #[test]
    fn extract_package_from_backtick_msg() {
        fn pkg(msg: &str) -> String {
            msg.split('`').nth(1).unwrap_or("unknown").to_string()
        }
        assert_eq!(pkg("Package `lodash` not found"), "lodash");
        assert_eq!(pkg("no backtick"), "unknown");
    }

    #[test]
    fn critical_paths_include_workflows_and_rbac() {
        assert!(is_critical_full_file_path(".github/workflows/ci.yml"));
        assert!(is_critical_full_file_path("src/auth/middleware.rs"));
        assert!(is_critical_full_file_path("k8s/rbac.yaml"));
        assert!(!is_critical_full_file_path("src/ui/button.tsx"));
    }
}
