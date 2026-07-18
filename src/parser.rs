use anyhow::Result;
use std::collections::HashMap;
use std::sync::LazyLock;

/// A parsed source file with imports and structure info
#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub path: String,
    pub language: String,
    pub raw_content: String,
    pub lines: Vec<SourceLine>,
    pub imports: Vec<Import>,
}

#[derive(Debug, Clone)]
pub struct SourceLine {
    pub number: usize,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub name: String,
    pub line: usize,
    pub column: usize,
}

pub fn parse_file(path: &str, content: &str) -> Result<ParsedFile> {
    let language = detect_language(path);
    let lines = content
        .lines()
        .enumerate()
        .map(|(index, content)| SourceLine {
            number: index + 1,
            content: content.to_string(),
        })
        .collect();

    Ok(parse_lines(path, language, content.to_string(), lines))
}

/// Parse a unified diff patch as the new-file side of the diff.
///
/// GitHub's pull-file response contains a unified diff, not source code. Passing
/// it directly to source detectors prefixes additions with `+` and reports patch
/// offsets as source lines. This function removes diff metadata/deletions and
/// preserves the new-file line number from each hunk, so findings can be posted
/// back as valid inline review comments.
pub fn parse_unified_diff(path: &str, patch: &str) -> Result<ParsedFile> {
    let mut lines = Vec::new();
    let mut next_line = None;

    for line in patch.lines() {
        if line.starts_with("@@") {
            next_line = parse_new_hunk_start(line);
            continue;
        }

        let Some(line_number) = next_line else {
            continue;
        };

        match line.as_bytes().first() {
            Some(b'+') if !line.starts_with("+++") => {
                lines.push(SourceLine {
                    number: line_number,
                    content: line[1..].to_string(),
                });
                next_line = Some(line_number + 1);
            }
            Some(b' ') => {
                lines.push(SourceLine {
                    number: line_number,
                    content: line[1..].to_string(),
                });
                next_line = Some(line_number + 1);
            }
            Some(b'-') | Some(b'\\') => {}
            _ => {}
        }
    }

    if lines.is_empty() {
        return parse_file(path, patch);
    }

    let language = detect_language(path);
    let raw_content = lines
        .iter()
        .map(|line| line.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(parse_lines(path, language, raw_content, lines))
}

fn parse_new_hunk_start(header: &str) -> Option<usize> {
    let new_range = header.split_whitespace().nth(2)?;
    new_range.strip_prefix('+')?.split(',').next()?.parse().ok()
}

fn parse_lines(
    path: &str,
    language: String,
    raw_content: String,
    lines: Vec<SourceLine>,
) -> ParsedFile {
    let imports = extract_imports(&language, &lines);

    ParsedFile {
        path: path.to_string(),
        language,
        raw_content,
        lines,
        imports,
    }
}

fn detect_language(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "js" => "javascript",
        "jsx" => "jsx",
        "ts" => "typescript",
        "tsx" => "tsx",
        "py" => "python",
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "rb" => "ruby",
        "php" => "php",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" | "cxx" => "cpp",
        "cs" => "csharp",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        _ => "unknown",
    }
    .to_string()
}

/// Extract import statements from parsed lines using regex patterns per language
fn extract_imports(language: &str, lines: &[SourceLine]) -> Vec<Import> {
    let patterns = IMPORT_PATTERNS
        .get(language)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let mut imports = Vec::new();

    for line in lines {
        for pattern in patterns {
            if let Some(caps) = pattern.regex.captures(&line.content) {
                let import_name = caps
                    .name("pkg")
                    .map(|m| m.as_str())
                    .or_else(|| caps.get(1).map(|m| m.as_str()))
                    .unwrap_or("");

                if !import_name.is_empty() {
                    let col = line.content.find(import_name).unwrap_or(0);
                    imports.push(Import {
                        name: import_name.to_string(),
                        line: line.number,
                        column: col,
                    });
                    break; // one import per line, skip remaining patterns
                }
            }
        }
    }

    imports
}

struct ImportPattern {
    regex: regex::Regex,
}

impl ImportPattern {
    fn new(pattern: &str) -> Self {
        Self {
            regex: regex::Regex::new(pattern).expect("invalid import regex"),
        }
    }
}

static IMPORT_PATTERNS: LazyLock<HashMap<&'static str, Vec<ImportPattern>>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("javascript", vec![
        ImportPattern::new(r#"(?:import|export)\s+(?:\w+\s*,?\s*)?\{\s*[\w\s,]*\}\s*from\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s+\*\s+as\s+\w+\s+from\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"(?:const|let|var)\s+\w+\s*=\s*require\s*\(\s*['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s*\(\s*['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s+\w+\s+from\s+['"](?P<pkg>[^'"]+)['"]"#),
    ]);
    m.insert("jsx", vec![
        ImportPattern::new(r#"(?:import|export)\s+(?:\w+\s*,?\s*)?\{\s*[\w\s,]*\}\s*from\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s+\*\s+as\s+\w+\s+from\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"(?:const|let|var)\s+\w+\s*=\s*require\s*\(\s*['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s*\(\s*['"](?P<pkg>[^'']+)['"]"#),
        ImportPattern::new(r#"import\s+\w+\s+from\s+['"](?P<pkg>[^'"]+)['"]"#),
    ]);
    m.insert("typescript", vec![
        ImportPattern::new(r#"(?:import|export)\s+(?:\w+\s*,?\s*)?\{\s*[\w\s,]*\}\s*from\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s+\*\s+as\s+\w+\s+from\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"(?:const|let|var)\s+\w+\s*=\s*require\s*\(\s*['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s*\(\s*['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s+\w+\s+from\s+['"](?P<pkg>[^'"]+)['"]"#),
    ]);
    m.insert("tsx", vec![
        ImportPattern::new(r#"(?:import|export)\s+(?:\w+\s*,?\s*)?\{\s*[\w\s,]*\}\s*from\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s+\*\s+as\s+\w+\s+from\s+['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"(?:const|let|var)\s+\w+\s*=\s*require\s*\(\s*['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s*\(\s*['"](?P<pkg>[^'"]+)['"]"#),
        ImportPattern::new(r#"import\s+\w+\s+from\s+['"](?P<pkg>[^'"]+)['"]"#),
    ]);
    m.insert(
        "python",
        vec![
            ImportPattern::new(r"^\s*import\s+(?P<pkg>\w+)"),
            ImportPattern::new(r"^\s*from\s+(?P<pkg>[\w.]+)\s+import"),
        ],
    );
    m.insert(
        "rust",
        vec![
            ImportPattern::new(r"^\s*use\s+(?:::)?(?P<pkg>[\w:]+)"),
            ImportPattern::new(r"^\s*extern\s+crate\s+(?P<pkg>\w+)"),
        ],
    );
    m.insert(
        "go",
        vec![
            ImportPattern::new(r#"^\s*import\s+["](?P<pkg>[^"]+)["]"#),
            ImportPattern::new(r#"^\s*import\s+\w+\s+["](?P<pkg>[^"]+)["]"#),
        ],
    );
    m.insert(
        "java",
        vec![ImportPattern::new(
            r"^\s*import\s+(?:static\s+)?(?P<pkg>[\w.]+);",
        )],
    );
    m.insert(
        "ruby",
        vec![
            ImportPattern::new(r#"^\s*require\s+['"](?P<pkg>[^'"]+)['"]"#),
            ImportPattern::new(r#"^\s*require_relative\s+['"](?P<pkg>[^'"]+)['"]"#),
            ImportPattern::new(r#"^\s*gem\s+['"](?P<pkg>[^'"]+)['"]"#),
        ],
    );
    m.insert(
        "php",
        vec![ImportPattern::new(r"^\s*use\s+(?P<pkg>[\w\\]+);")],
    );
    m
});

pub fn supported_languages() -> &'static [&'static str] {
    &[
        "javascript",
        "typescript",
        "jsx",
        "tsx",
        "python",
        "rust",
        "go",
        "java",
        "ruby",
        "php",
        "csharp",
        "kotlin",
        "swift",
        "scala",
    ]
}

pub fn is_supported(path: &str) -> bool {
    let lang = detect_language(path);
    supported_languages().contains(&lang.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("file.rs"), "rust");
        assert_eq!(detect_language("file.py"), "python");
        assert_eq!(detect_language("file.ts"), "typescript");
        assert_eq!(detect_language("file.js"), "javascript");
        assert_eq!(detect_language("file.go"), "go");
        assert_eq!(detect_language("file.java"), "java");
        assert_eq!(detect_language("file.unknown"), "unknown");
    }

    #[test]
    fn test_parse_js_imports() {
        let content = r#"import React from 'react';
import { useState } from 'react';
import fs from 'node:fs';
const express = require('express');"#;
        let parsed = parse_file("test.js", content).unwrap();
        assert_eq!(parsed.language, "javascript");
        assert!(!parsed.imports.is_empty(), "should extract imports");
        let names: Vec<&str> = parsed.imports.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"react"), "should find react import");
    }

    #[test]
    fn test_parse_python_imports() {
        let content = "import os\nfrom datetime import datetime\nimport numpy as np";
        let parsed = parse_file("test.py", content).unwrap();
        assert_eq!(parsed.language, "python");
        let names: Vec<&str> = parsed.imports.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"os"), "should find os import");
        assert!(names.contains(&"datetime"), "should find datetime import");
    }

    #[test]
    fn test_parse_rust_imports() {
        let content = "use std::collections::HashMap;\nuse serde::{Deserialize, Serialize};";
        let parsed = parse_file("test.rs", content).unwrap();
        assert_eq!(parsed.language, "rust");
        assert!(!parsed.imports.is_empty(), "should extract rust imports");
    }

    #[test]
    fn test_unsupported_file() {
        assert!(!is_supported("file.bin"));
        assert!(is_supported("file.rs"));
        assert!(is_supported("file.py"));
    }

    #[test]
    fn test_known_good_imports_from_diff_content() {
        let parsed = parse_file("test.js", "import React from 'react';").unwrap();
        assert_eq!(parsed.path, "test.js");
        assert!(!parsed.imports.is_empty());
        assert!(parsed.imports.iter().any(|i| i.name == "react"));
    }

    #[test]
    fn test_parse_unified_diff_uses_new_file_line_numbers() {
        let patch = "@@ -8,3 +8,4 @@\n unchanged();\n-old_call();\n+import { tool } from 'package';\n+new_call();\n@@ -20,0 +22,1 @@\n+const value = tool();";
        let parsed = parse_unified_diff("src/example.ts", patch).unwrap();
        let import = parsed.imports.first().expect("import should be found");

        assert_eq!(import.name, "package");
        assert_eq!(import.line, 9);
        assert!(!parsed.raw_content.contains("@@"));
        assert!(!parsed.raw_content.contains("+import"));
        assert!(parsed.lines.iter().any(|line| line.number == 22));
    }
}
