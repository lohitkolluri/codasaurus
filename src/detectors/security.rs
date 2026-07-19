use crate::detectors::Finding;
use crate::parser::ParsedFile;
use aho_corasick::AhoCorasick;
use regex::Regex;
use std::sync::LazyLock;

static TODO_RE: LazyLock<AhoCorasick> = LazyLock::new(|| {
    AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build(["todo", "fixme", "xxx", "hack"])
        .expect("valid TODO patterns")
});

// Lines matching these patterns (word-boundary checked) skip secret detection.
// Avoids false positives from substrings like "contest" matching "test_".
static SKIP_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\btest_\b|\bmock_\b|\bfixture\b").expect("valid skip line regex")
});

static SECRET_PRE_CHECK: LazyLock<AhoCorasick> = LazyLock::new(|| {
    AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build([
            "key",
            "token",
            "secret",
            "password",
            "bearer",
            "aws_",
            "ghp_",
            "gho_",
            "ghs_",
            "-----begin",
            "eyj",
            "mongodb",
            "postgresql",
            "mysql",
            "redis://",
            "api_key",
            "apikey",
            "passwd",
            "pwd",
            "xoxb",
        ])
        .expect("valid secret pre-check patterns")
});

pub fn detect_secrets(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for file in parsed_files {
        for line in &file.lines {
            let trimmed = line.content.trim();

            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            // Fast pre-filter: skip lines without any secret-like keywords
            if !SECRET_PRE_CHECK.is_match(trimmed) {
                continue;
            }

            // Skip lines that are inside string literal context (test fixtures, example code, mocks)
            if is_in_string_context(trimmed) {
                continue;
            }
            // Skip test/mock/fixture lines — word-boundary checked to avoid
            // false positives from substrings like "contest" matching "test_".
            if SKIP_LINE_RE.is_match(trimmed) {
                continue;
            }

            for pattern in SECRET_PATTERNS.iter() {
                if let Some(captures) = pattern.regex.captures(trimmed) {
                    let value = captures.get(1).map(|m| m.as_str()).unwrap_or("");
                    let masked = mask_value(value);

                    findings.push(Finding {
                        detector: "secrets".to_string(),
                        severity: "blocking",
                        file: file.path.clone(),
                        line: line.number,
                        column: 0,
                        message: format!("Potential {} detected: `{}`", pattern.name, masked),
                        suggestion: Some(format!(
                            "Remove this {} and use environment variables instead.",
                            pattern.name
                        )),
                        evidence: Some(format!("`{masked}`")),
                        codemod: None,
                    });
                }
            }
        }
    }

    findings
}

/// Heuristic: skip lines that are clearly within string literal context.
/// This reduces false positives from test fixtures and example code.
fn is_in_string_context(line: &str) -> bool {
    let trimmed = line.trim();
    // Entire line wrapped in quotes: "const API_KEY = ..." or '...'
    // BUT: don't skip JSON key-value lines like "api_key": "secret"
    // which would falsely hide real secrets in config files.
    let is_quoted_line = (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''));
    if is_quoted_line {
        // A line containing `: "` or `: '` is likely a config key-value pair.
        if trimmed.contains(": \"") || trimmed.contains(": '") {
            return false;
        }
        return true;
    }
    // Line starts in a comment context containing example/sample keywords
    if (trimmed.starts_with("// ") || trimmed.starts_with("# ") || trimmed.starts_with("<!--"))
        && (trimmed[3..].to_ascii_lowercase().contains("example")
            || trimmed[3..].to_ascii_lowercase().contains("sample"))
    {
        return true;
    }
    false
}

pub fn detect_todos(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for file in parsed_files {
        for line in &file.lines {
            let trimmed = line.content.trim();

            if TODO_RE.is_match(trimmed) {
                // Skip markdown heading references to TODO
                if trimmed.starts_with("##")
                    && trimmed
                        .as_bytes()
                        .windows(4)
                        .any(|w| w.eq_ignore_ascii_case(b"todo"))
                {
                    continue;
                }

                // Skip string-literal context to reduce false positives
                if is_in_string_context(trimmed) {
                    continue;
                }

                findings.push(Finding {
                    detector: "todo-leaks".to_string(),
                    severity: "warning",
                    file: file.path.clone(),
                    line: line.number,
                    column: 0,
                    message: format!(
                        "Leftover placeholder or incomplete code: \"{}\"",
                        trimmed.chars().take(80).collect::<String>()
                    ),
                    suggestion: Some(
                        "Complete the implementation or remove the placeholder.".to_string(),
                    ),
                    evidence: Some(trimmed.chars().take(120).collect()),
                    codemod: None,
                });
            }
        }
    }

    findings
}

fn mask_value(value: &str) -> String {
    let len = value.len();
    if len <= 8 {
        return "***".to_string();
    }
    format!("*** ({len} chars)")
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

static SECRET_PATTERNS: LazyLock<Vec<SecretPattern>> = LazyLock::new(|| {
    vec![
        SecretPattern::new(
            "AWS Access Key",
            r#"(?i)(?:aws_access_key_id|AWS_ACCESS_KEY|AKIA[0-9A-Z]{16,}|AWS_KEY)\s*[:=]\s*['"]?([A-Za-z0-9/+=]{20,})['"]?"#,
        ),
        SecretPattern::new(
            "AWS Secret Key",
            r#"(?i)(?:aws_secret_access_key|AWS_SECRET_KEY)\s*[:=]\s*['"]?([A-Za-z0-9/+=]{40,})['\"]?"#,
        ),
        SecretPattern::new(
            "GitHub Token",
            r"(?i)(?:ghp_|gho_|ghu_|ghs_|ghr_)[A-Za-z0-9_]{36,}",
        ),
        SecretPattern::new(
            "API Key",
            r#"(?i)(?:api[_-]?key|apikey|api_secret|api_secret_key)\s*[:=]\s*['"]?([A-Za-z0-9_\-]{16,})['"]?"#,
        ),
        SecretPattern::new("Bearer Token", r"(?i)bearer\s+[A-Za-z0-9_\-\.]{20,}"),
        SecretPattern::new("Slack Token", r"xox[baprs]-[0-9a-z-]{10,}"),
        SecretPattern::new(
            "Private Key",
            r"-----BEGIN\s?(?:RSA|DSA|EC|OPENSSH|PRIVATE)\s?KEY-----",
        ),
        SecretPattern::new(
            "JWT Token",
            r"eyJ[A-Za-z0-9_\-]{1,200}\.[A-Za-z0-9_\-]{1,200}\.[A-Za-z0-9_\-]{1,200}",
        ),
        SecretPattern::new(
            "Password",
            r#"(?i)(?:password|passwd|pwd)\s*[:=]\s*['"]?([^'"\s]{8,})['"]?"#,
        ),
        SecretPattern::new(
            "Connection String",
            r"(?i)(?:mongodb|postgresql|mysql|redis)://[^\s]{10,}",
        ),
    ]
});
