use crate::detectors::Finding;
use crate::parser::ParsedFile;

/// Detect potential secrets and credentials in code
pub fn detect_secrets(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for file in parsed_files {
        for line in &file.lines {
            let trimmed = line.content.trim();

            // Skip comments and empty lines
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            for pattern in SECRET_PATTERNS.iter() {
                if let Some(captures) = pattern.regex.captures(trimmed) {
                    let value = captures.get(1).map(|m| m.as_str()).unwrap_or("");
                    let masked = mask_value(value);

                    findings.push(Finding {
                        detector: "secrets".to_string(),
                        severity: "blocking".to_string(),
                        file: file.path.clone(),
                        line: line.number,
                        column: 0,
                        message: format!("Potential {} detected: `{}`", pattern.name, masked),
                        suggestion: Some(format!(
                            "Remove this {} and use environment variables instead.",
                            pattern.name
                        )),
                        evidence: Some(format!("`{}`", masked)),
                        codemod: None,
                    });
                }
            }
        }
    }

    findings
}

/// Detect TODO/FIXME placeholders left by AI or developers
pub fn detect_todos(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for file in parsed_files {
        for line in &file.lines {
            let trimmed = line.content.trim();
            let lower = trimmed.to_ascii_lowercase();

            if lower.contains("todo")
                || lower.contains("fixme")
                || lower.contains("xxx")
                || lower.contains("hack")
            {
                // Skip if it's just a reference in documentation
                if trimmed.starts_with('#') && trimmed.contains("todo") {
                    continue;
                }

                findings.push(Finding {
                    detector: "todo-leaks".to_string(),
                    severity: "warning".to_string(),
                    file: file.path.clone(),
                    line: line.number,
                    column: 0,
                    message: format!(
                        "Leftover placeholder or incomplete code: \"{}\"",
                        trimmed.chars().take(80).collect::<String>()
                    ),
                    suggestion: Some("Complete the implementation or remove the placeholder.".to_string()),
                    evidence: Some(trimmed.chars().take(120).collect()),
                    codemod: None,
                });
            }
        }
    }

    findings
}

fn mask_value(value: &str) -> String {
    // Must not panic on multi-byte UTF-8 — safe character-level slicing
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        return "***".to_string();
    }
    let prefix: String = chars.iter().take(4).collect();
    let suffix: String = chars.iter().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{}...{}", prefix, suffix)
}

struct SecretPattern {
    name: &'static str,
    regex: regex::Regex,
}

impl SecretPattern {
    fn new(name: &'static str, pattern: &str) -> Self {
        Self {
            name,
            regex: regex::Regex::new(pattern).expect("invalid regex pattern"),
        }
    }
}

use once_cell::sync::Lazy;

static SECRET_PATTERNS: Lazy<Vec<SecretPattern>> = Lazy::new(|| {
    vec![
        SecretPattern::new("AWS Access Key", r#"(?i)(?:aws_access_key_id|AWS_ACCESS_KEY|AKIA[0-9A-Z]{16,}|AWS_KEY)\s*[:=]\s*['"]?([A-Za-z0-9/+=]{20,})['"]?"#),
        SecretPattern::new("AWS Secret Key", r#"(?i)(?:aws_secret_access_key|AWS_SECRET_KEY)\s*[:=]\s*['"]?([A-Za-z0-9/+=]{40,})['\"]?"#),
        SecretPattern::new("GitHub Token", r"(?i)(?:ghp_|gho_|ghu_|ghs_|ghr_)[A-Za-z0-9_]{36,}"),
        SecretPattern::new("API Key", r#"(?i)(?:api[_-]?key|apikey|api_secret|api_secret_key)\s*[:=]\s*['"]?([A-Za-z0-9_\-]{16,})['"]?"#),
        SecretPattern::new("Bearer Token", r"(?i)bearer\s+[A-Za-z0-9_\-\.]{20,}"),
        SecretPattern::new("Slack Token", r"xox[baprs]-[0-9a-z-]{10,}"),
        SecretPattern::new("Private Key", r"-----BEGIN\s?(?:RSA|DSA|EC|OPENSSH|PRIVATE)\s?KEY-----"),
        SecretPattern::new("JWT Token", r"eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+"),
        SecretPattern::new("Password", r#"(?i)(?:password|passwd|pwd)\s*[:=]\s*['"]?([^'"\s]{8,})['"]?"#),
        SecretPattern::new("Connection String", r"(?i)(?:mongodb|postgresql|mysql|redis)://[^\s]{10,}"),
    ]
});
