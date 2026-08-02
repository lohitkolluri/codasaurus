//! IaC detectors: Terraform and Kubernetes YAML red flags.

use crate::detectors::Finding;
use crate::parser::ParsedFile;

pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut out = Vec::new();
    for file in parsed_files {
        let path = file.path.to_lowercase();
        if path.ends_with(".tf") || path.ends_with(".tfvars") {
            out.extend(scan_terraform(file));
        } else if path.ends_with(".yaml")
            || path.ends_with(".yml")
            || path.contains("kustomization")
            || path.contains("/deploy/")
            || path.contains("/helm/")
        {
            out.extend(scan_k8s(file));
        }
    }
    out
}

fn scan_terraform(file: &ParsedFile) -> Vec<Finding> {
    let mut findings = Vec::new();
    let content = &file.raw_content;
    for (idx, line) in content.lines().enumerate() {
        let line_no = idx + 1;
        let lower = line.to_lowercase();
        if lower.contains("0.0.0.0/0") || lower.contains("::/0") {
            findings.push(Finding {
                detector: "iac".into(),
                severity: "warning",
                file: file.path.clone(),
                line: line_no,
                column: 0,
                message: "Open CIDR `0.0.0.0/0` (or `::/0`) in Terraform — review exposure".into(),
                suggestion: Some("Restrict ingress to known CIDRs or security groups.".into()),
                evidence: Some(line.trim().chars().take(120).collect()),
                codemod: None,
                confidence: None,
                judge_rationale: None,
                reachability: None,
            });
        }
        if lower.contains("password")
            && (lower.contains('=') || lower.contains(':'))
            && !lower.contains("var.")
            && !lower.contains("sensitive")
            && line.contains('"')
        {
            findings.push(Finding {
                detector: "iac".into(),
                severity: "blocking",
                file: file.path.clone(),
                line: line_no,
                column: 0,
                message: "Possible plaintext secret/password in Terraform".into(),
                suggestion: Some("Use secrets manager / sensitive variables.".into()),
                evidence: Some(line.trim().chars().take(80).collect()),
                codemod: None,
                confidence: None,
                judge_rationale: None,
                reachability: None,
            });
        }
        if lower.contains("resource \"aws_security_group\"") {
            findings.push(Finding {
                detector: "iac".into(),
                severity: "info",
                file: file.path.clone(),
                line: line_no,
                column: 0,
                message: "Security group defined — verify ingress is least-privilege".into(),
                suggestion: None,
                evidence: None,
                codemod: None,
                confidence: None,
                judge_rationale: None,
                reachability: None,
            });
        }
    }
    findings.truncate(20);
    findings
}

fn scan_k8s(file: &ParsedFile) -> Vec<Finding> {
    let mut findings = Vec::new();
    let content = &file.raw_content;
    let lower_all = content.to_lowercase();
    if !(lower_all.contains("apiversion:") || lower_all.contains("kind:")) {
        return findings;
    }
    for (idx, line) in content.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim().to_lowercase();
        if trimmed == "privileged: true" {
            findings.push(Finding {
                detector: "iac".into(),
                severity: "blocking",
                file: file.path.clone(),
                line: line_no,
                column: 0,
                message: "Kubernetes container runs as privileged".into(),
                suggestion: Some("Remove privileged: true unless absolutely required.".into()),
                evidence: Some(line.trim().into()),
                codemod: None,
                confidence: None,
                judge_rationale: None,
                reachability: None,
            });
        }
        if trimmed == "hostnetwork: true" {
            findings.push(Finding {
                detector: "iac".into(),
                severity: "warning",
                file: file.path.clone(),
                line: line_no,
                column: 0,
                message: "Pod uses hostNetwork".into(),
                suggestion: Some("Avoid hostNetwork for untrusted workloads.".into()),
                evidence: Some(line.trim().into()),
                codemod: None,
                confidence: None,
                judge_rationale: None,
                reachability: None,
            });
        }
        if trimmed.starts_with("value:")
            && (trimmed.contains("password")
                || trimmed.contains("secret")
                || trimmed.contains("token")
                || trimmed.contains("apikey"))
        {
            findings.push(Finding {
                detector: "iac".into(),
                severity: "warning",
                file: file.path.clone(),
                line: line_no,
                column: 0,
                message: "Possible secret embedded as literal env value".into(),
                suggestion: Some("Use Secret refs / external secrets instead of literals.".into()),
                evidence: Some(line.trim().chars().take(80).collect()),
                codemod: None,
                confidence: None,
                judge_rationale: None,
                reachability: None,
            });
        }
    }
    findings.truncate(20);
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_open_cidr() {
        let f = crate::parser::parse_file("net.tf", "cidr_blocks = [\"0.0.0.0/0\"]\n").unwrap();
        let hits = detect(&[f]);
        assert!(hits.iter().any(|h| h.message.contains("0.0.0.0/0")));
    }

    #[test]
    fn flags_privileged_pod() {
        let f = crate::parser::parse_file(
            "deploy/pod.yaml",
            "apiVersion: v1\nkind: Pod\nprivileged: true\n",
        )
        .unwrap();
        let hits = detect(&[f]);
        assert!(hits.iter().any(|h| h.severity == "blocking"));
    }
}
