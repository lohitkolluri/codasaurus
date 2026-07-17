use anyhow::Result;
use std::collections::HashSet;

/// A parsed source file with imports and structure info
#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub path: String,
    pub language: String,
    pub raw_content: String,
    pub lines: Vec<SourceLine>,
    pub imports: Vec<Import>,
}

/// A single line in a source file
#[derive(Debug, Clone)]
pub struct SourceLine {
    pub number: usize,
    pub content: String,
}

/// An import statement
#[derive(Debug, Clone)]
pub struct Import {
    pub name: String,
    pub line: usize,
    pub column: usize,
}

/// Parse a file and extract structured information
pub fn parse_file(path: &str, content: &str) -> Result<ParsedFile> {
    let language = detect_language(path);
    let lines: Vec<SourceLine> = content
        .lines()
        .enumerate()
        .map(|(i, l)| SourceLine {
            number: i + 1,
            content: l.to_string(),
        })
        .collect();

    let imports = extract_imports(&language, &lines);

    Ok(ParsedFile {
        path: path.to_string(),
        language,
        raw_content: content.to_string(),
        lines,
        imports,
    })
}

/// Detect language from file extension
fn detect_language(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "js" => "javascript".to_string(),
        "jsx" => "jsx".to_string(),
        "ts" => "typescript".to_string(),
        "tsx" => "tsx".to_string(),
        "py" => "python".to_string(),
        "rs" => "rust".to_string(),
        "go" => "go".to_string(),
        "java" => "java".to_string(),
        "rb" => "ruby".to_string(),
        "php" => "php".to_string(),
        "c" | "h" => "c".to_string(),
        "cpp" | "hpp" | "cc" | "cxx" => "cpp".to_string(),
        "cs" => "csharp".to_string(),
        "swift" => "swift".to_string(),
        "kt" | "kts" => "kotlin".to_string(),
        "scala" => "scala".to_string(),
        _ => "unknown".to_string(),
    }
}

/// Extract import statements from parsed lines using regex patterns per language
fn extract_imports(language: &str, lines: &[SourceLine]) -> Vec<Import> {
    let patterns = get_import_patterns(language);
    let mut imports = Vec::new();

    for line in lines {
        for pattern in &patterns {
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
                }
            }
        }
    }

    imports
}

struct ImportPattern {
    name: &'static str,
    regex: regex::Regex,
}

impl ImportPattern {
    fn new(name: &'static str, pattern: &str) -> Self {
        Self {
            name,
            regex: regex::Regex::new(pattern).expect("invalid import regex"),
        }
    }
}

fn get_import_patterns(language: &str) -> Vec<ImportPattern> {
    match language {
        "javascript" | "jsx" | "typescript" | "tsx" => vec![
            // import x from 'y'
            ImportPattern::new("esm-default", r#"(?:import|export)\s+(?:\w+\s*,?\s*)?\{\s*[\w\s,]*\}\s*from\s+['"](?P<pkg>[^'"]+)['"]"#),
            // import 'y'
            ImportPattern::new("esm-side-effect", r#"import\s+['"](?P<pkg>[^'"]+)['"]"#),
            // import * as x from 'y'
            ImportPattern::new("esm-ns", r#"import\s+\*\s+as\s+\w+\s+from\s+['"](?P<pkg>[^'"]+)['"]"#),
            // require('y')
            ImportPattern::new("cjs", r#"(?:const|let|var)\s+\w+\s*=\s*require\s*\(\s*['"](?P<pkg>[^'"]+)['"]"#),
            // dynamic import('y')
            ImportPattern::new("dynamic", r#"import\s*\(\s*['"](?P<pkg>[^'"]+)['"]"#),
            // import x from 'y' (default import)
            ImportPattern::new("esm-default2", r#"import\s+\w+\s+from\s+['"](?P<pkg>[^'"]+)['"]"#),
        ],
        "python" => vec![
            // import x
            ImportPattern::new("import", r"^\s*import\s+(?P<pkg>\w+)"),
            // from x import y
            ImportPattern::new("from-import", r"^\s*from\s+(?P<pkg>[\w.]+)\s+import"),
        ],
        "rust" => vec![
            // use crate::module
            ImportPattern::new("use", r"^\s*use\s+(?:::)?(?P<pkg>[\w:]+)"),
            // extern crate x
            ImportPattern::new("extern-crate", r"^\s*extern\s+crate\s+(?P<pkg>\w+)"),
        ],
        "go" => vec![
            // import "pkg"
            ImportPattern::new("import", r#"^\s*import\s+["](?P<pkg>[^"]+)["]"#),
            // import alias "pkg"
            ImportPattern::new("import-alias", r#"^\s*import\s+\w+\s+["](?P<pkg>[^"]+)["]"#),
        ],
        "java" => vec![
            // import com.example.X
            ImportPattern::new("import", r"^\s*import\s+(?:static\s+)?(?P<pkg>[\w.]+);"),
        ],
        "ruby" => vec![
            // require 'x'
            ImportPattern::new("require", r#"^\s*require\s+['"](?P<pkg>[^'"]+)['"]"#),
            // require_relative 'x'
            ImportPattern::new("require-rel", r#"^\s*require_relative\s+['"](?P<pkg>[^'"]+)['"]"#),
            // gem 'x'
            ImportPattern::new("gem", r#"^\s*gem\s+['"](?P<pkg>[^'"]+)['"]"#),
        ],
        "php" => vec![
            // use Vendor\Package
            ImportPattern::new("use", r"^\s*use\s+(?P<pkg>[\w\\]+);"),
        ],
        _ => vec![],
    }
}

/// Supported languages for checking
pub fn supported_languages() -> Vec<&'static str> {
    vec![
        "javascript", "typescript", "jsx", "tsx",
        "python", "rust", "go", "java",
        "ruby", "php", "csharp", "kotlin",
        "swift", "scala",
    ]
}

/// Check if a file extension is parseable
pub fn is_supported(path: &str) -> bool {
    let lang = detect_language(path);
    supported_languages().contains(&lang.as_str())
}
