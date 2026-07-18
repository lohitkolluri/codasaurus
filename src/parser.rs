use anyhow::Result;
use std::sync::LazyLock;
use std::collections::HashMap;

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
    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();
    let mut lines = Vec::with_capacity(total_lines);
    for (i, l) in all_lines.into_iter().enumerate() {
        lines.push(SourceLine {
            number: i + 1,
            content: l.to_string(),
        });
    }

    let imports = extract_imports(&language, &lines);

    Ok(ParsedFile {
        path: path.to_string(),
        language,
        raw_content: content.to_string(),
        lines,
        imports,
    })
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
            ImportPattern::new(
                r#"^\s*require_relative\s+['"](?P<pkg>[^'"]+)['"]"#,
            ),
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
}
