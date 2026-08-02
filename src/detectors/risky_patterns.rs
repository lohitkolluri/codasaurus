//! Pattern-based risky API / injection heuristics (Tier-1, no LLM).

use crate::detectors::Finding;
use crate::parser::ParsedFile;
use regex::Regex;
use std::sync::LazyLock;

struct RiskyPattern {
    detector_tag: &'static str,
    severity: &'static str,
    regex: Regex,
    message: &'static str,
    suggestion: &'static str,
}

impl RiskyPattern {
    fn new(
        detector_tag: &'static str,
        severity: &'static str,
        pattern: &str,
        message: &'static str,
        suggestion: &'static str,
    ) -> Self {
        Self {
            detector_tag,
            severity,
            regex: Regex::new(pattern).expect("valid risky-pattern regex"),
            message,
            suggestion,
        }
    }
}

static RISKY: LazyLock<Vec<RiskyPattern>> = LazyLock::new(|| {
    vec![
        RiskyPattern::new(
            "eval",
            "blocking",
            r"\beval\s*\(",
            "Use of eval() is a code-injection risk.",
            "Avoid eval; parse data with JSON.parse or a safer interpreter.",
        ),
        RiskyPattern::new(
            "new-function",
            "warning",
            r"\bnew\s+Function\s*\(",
            "new Function(...) is equivalent to eval for attacker-controlled input.",
            "Prefer static functions or a sandboxed expression library.",
        ),
        RiskyPattern::new(
            "child-process-exec",
            "blocking",
            r"\b(exec|execSync)\s*\(.*\$\{",
            "Shell exec with template interpolation can enable command injection.",
            "Use execFile/spawn with an args array; never interpolate into a shell string.",
        ),
        RiskyPattern::new(
            "python-os-system",
            "blocking",
            r"\bos\.system\s*\(",
            "os.system passes a shell string — injection risk if input is untrusted.",
            "Use subprocess.run with a list of args.",
        ),
        RiskyPattern::new(
            "python-pickle",
            "warning",
            r"\bpickle\.loads?\s*\(",
            "Unpickling untrusted data can execute arbitrary code.",
            "Prefer JSON or a safe serializer; never pickle user input.",
        ),
        RiskyPattern::new(
            "dangerously-set-inner-html",
            "blocking",
            "dangerouslySetInnerHTML",
            "dangerouslySetInnerHTML can introduce XSS if content is untrusted.",
            "Sanitize with a vetted library or avoid raw HTML.",
        ),
        RiskyPattern::new(
            "innerhtml-assign",
            "warning",
            r"\.innerHTML\s*=",
            "Assigning to innerHTML with untrusted data enables XSS.",
            "Use textContent or a sanitizer (DOMPurify).",
        ),
        RiskyPattern::new(
            "sql-string-concat",
            "warning",
            r"(?i)(SELECT|INSERT|UPDATE|DELETE).{0,80}(\+|format!)",
            "SQL built via string concatenation/format may be injectable.",
            "Use parameterized queries / bound arguments.",
        ),
        RiskyPattern::new(
            "tls-insecure",
            "blocking",
            r"(?i)(InsecureSkipVerify\s*:\s*true|rejectUnauthorized\s*:\s*false|verify\s*=\s*False)",
            "TLS certificate verification is disabled.",
            "Enable certificate verification in production.",
        ),
        RiskyPattern::new(
            "md5-sha1",
            "info",
            r"(?i)\b(md5|sha1)\s*\(",
            "MD5/SHA-1 are weak for security-sensitive hashing.",
            "Use SHA-256+ or a password KDF (argon2/bcrypt) as appropriate.",
        ),
        RiskyPattern::new(
            "hardcoded-jwt-none",
            "blocking",
            r"(?i)algorithm\s*[:=]\s*.{0,3}none",
            "JWT alg: none disables signature verification.",
            "Use a strong signing algorithm (RS256/ES256/HS256) and verify signatures.",
        ),
        RiskyPattern::new(
            "cors-star",
            "warning",
            r"(?i)Access-Control-Allow-Origin.{0,12}\*",
            "CORS * with credentials (or sensitive APIs) is often unsafe.",
            "Reflect an allowlist of origins instead of *.",
        ),
    ]
});

/// Detect common insecure / injection patterns in changed lines.
pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in parsed_files {
        let lower = file.path.to_ascii_lowercase();
        if lower.ends_with(".md") || lower.ends_with(".mdx") {
            continue;
        }
        for line in &file.lines {
            let trimmed = line.content.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }
            for pattern in RISKY.iter() {
                if pattern.regex.is_match(trimmed) {
                    findings.push(Finding {
                        detector: "risky-patterns".to_string(),
                        severity: pattern.severity,
                        file: file.path.clone(),
                        line: line.number,
                        column: 0,
                        message: format!("{} ({})", pattern.message, pattern.detector_tag),
                        suggestion: Some(pattern.suggestion.to_string()),
                        evidence: Some(trimmed.chars().take(160).collect()),
                        codemod: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{ParsedFile, SourceLine};

    fn file(path: &str, content: &str) -> ParsedFile {
        ParsedFile {
            path: path.into(),
            language: "javascript".into(),
            raw_content: content.into(),
            lines: vec![SourceLine {
                number: 1,
                content: content.into(),
            }],
            imports: vec![],
        }
    }

    #[test]
    fn catches_eval() {
        let f = detect(&[file("a.js", "const x = eval(userInput);")]);
        assert!(f.iter().any(|x| x.message.contains("eval")));
    }

    #[test]
    fn catches_dangerously_set_inner_html() {
        let f = detect(&[file("A.tsx", "<div dangerouslySetInnerHTML={html} />")]);
        assert!(!f.is_empty());
    }
}
