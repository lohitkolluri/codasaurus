use crate::detectors::Finding;
use crate::parser::ParsedFile;

pub fn detect_over_engineering(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for file in parsed_files {
        let content = &file.raw_content;
        let lines: Vec<&str> = content.lines().collect();

        if let Some(finding) = check_single_impl_interface(&file.path, &lines) {
            findings.push(finding);
        }

        if let Some(finding) = check_deep_nesting(&file.path, &lines) {
            findings.push(finding);
        }

        if let Some(finding) = check_unnecessary_factory(&file.path, content) {
            findings.push(finding);
        }

        if let Some(finding) = check_abstraction_overload(&file.path, content) {
            findings.push(finding);
        }
    }

    findings
}

pub fn detect_boilerplate(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for file in parsed_files {
        let content = &file.raw_content;
        let lines: Vec<&str> = content.lines().collect();
        let line_count = lines.len();

        if let Some(finding) = check_long_functions(&file.path, content) {
            findings.push(finding);
        }

        if let Some(finding) = check_repeated_code(&file.path, &lines) {
            findings.push(finding);
        }

        if line_count > 30 {
            let comment_ratio = count_comment_lines(&lines) as f64 / line_count as f64;
            if comment_ratio > 0.4 {
                findings.push(Finding {
                    detector: "boilerplate".to_string(),
                    severity: "warning",
                    file: file.path.clone(),
                    line: 0,
                    column: 0,
                    message: format!(
                        "File has {:.0}% comment lines — AI-generated code often over-comments.",
                        comment_ratio * 100.0
                    ),
                    suggestion: Some(
                        "Remove obvious comments that explain what the code does, keep only 'why' comments."
                            .to_string(),
                ),
                    evidence: None,
                    codemod: None,
                });
            }
        }

        if let Some(finding) = check_boilerplate_getters_setters(&file.path, content) {
            findings.push(finding);
        }
    }

    findings
}

fn check_single_impl_interface(path: &str, lines: &[&str]) -> Option<Finding> {
    let trait_count = lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            if t.starts_with("//") || t.starts_with('#') {
                return false;
            }
            t.contains("trait ") && (t.starts_with("pub") || t.starts_with("trait"))
        })
        .count();

    let impl_count = lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            if t.starts_with("//") || t.starts_with('#') {
                return false;
            }
            t.starts_with("impl ")
                || t.starts_with("pub impl ")
                || t.starts_with("unsafe impl ")
                || t.starts_with("pub unsafe impl ")
        })
        .count();

    if trait_count == 1 && impl_count == 1 {
        return Some(Finding {
            detector: "over-engineering".to_string(),
            severity: "warning",
            file: path.to_string(),
            line: 0,
            column: 0,
            message: "Interface/trait with only one implementation — unnecessary abstraction."
                .to_string(),
            suggestion: Some(
                "Remove the interface and use the concrete type directly. Add an interface only when you have multiple implementations."
                    .to_string(),
            ),
            evidence: None,
            codemod: None,
        });
    }

    None
}

fn check_deep_nesting(path: &str, lines: &[&str]) -> Option<Finding> {
    let mut max_depth = 0usize;
    let mut current_depth = 0usize;
    let mut deepest_line = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }

        let opens = trimmed.bytes().filter(|&b| b == b'{').count();
        let closes = trimmed.bytes().filter(|&b| b == b'}').count();

        // Calculate depth AFTER this line
        if opens > closes {
            current_depth += opens - closes;
        } else if closes > opens && current_depth > 0 {
            // Don't go below 0
            current_depth = current_depth.saturating_sub(closes - opens);
        }

        if current_depth > max_depth {
            max_depth = current_depth;
            deepest_line = i + 1;
        }
    }

    if max_depth > 6 {
        Some(Finding {
            detector: "over-engineering".to_string(),
            severity: "info",
            file: path.to_string(),
            line: deepest_line,
            column: 0,
            message: format!(
                "Deep nesting ({} levels) — AI code often creates unnecessarily nested structures.",
                max_depth
            ),
            suggestion: Some(
                "Extract nested logic into separate functions to improve readability.".to_string(),
            ),
            evidence: None,
            codemod: None,
        })
    } else {
        None
    }
}

fn check_unnecessary_factory(path: &str, content: &str) -> Option<Finding> {
    let factory_keywords = ["Factory", "factory", "Builder", "builder"];
    let has_factory = factory_keywords.iter().any(|k| content.contains(k));

    if !has_factory {
        return None;
    }

    // Count types/structs/classes that could be created directly
    let total_types = content
        .lines()
        .filter(|line| line.contains("struct ") || line.contains("class "))
        .count();

    // Factory with very few types is over-engineering
    if total_types <= 3 && has_factory {
        Some(Finding {
            detector: "over-engineering".to_string(),
            severity: "warning",
            file: path.to_string(),
            line: 0,
            column: 0,
            message: format!(
                "Factory/Builder pattern with only {} type(s) — unnecessary complexity.",
                total_types
            ),
            suggestion: Some(
                "Use constructors directly. Factory/Builder patterns add value with 5+ variants."
                    .to_string(),
            ),
            evidence: None,
            codemod: None,
        })
    } else {
        None
    }
}

fn check_abstraction_overload(path: &str, content: &str) -> Option<Finding> {
    let abstraction_keywords = [
        "AbstractFactory",
        "AbstractBase",
        "BaseHandler",
        "BaseService",
        "AbstractService",
        "GenericHandler",
        "HandlerInterface",
    ];

    let count = abstraction_keywords
        .iter()
        .filter(|k| content.contains(*k))
        .count();

    if count >= 2 {
        Some(Finding {
            detector: "over-engineering".to_string(),
            severity: "warning",
            file: path.to_string(),
            line: 0,
            column: 0,
            message: format!(
                "Multiple abstraction layers detected ({}) — AI code often over-abstracts.",
                count
            ),
            suggestion: Some(
                "Prefer concrete implementations. Add abstractions only when you have multiple implementations."
                    .to_string(),
            ),
            evidence: None,
            codemod: None,
        })
    } else {
        None
    }
}

fn check_long_functions(path: &str, content: &str) -> Option<Finding> {
    let mut long_funcs = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        let is_function = trimmed.contains("fn ")
            || trimmed.contains("def ")
            || trimmed.contains("function ")
            // `=>` alone is too broad (matches match arms, closures, array methods).
            // Only match `=>` when it follows a fn/def/function keyword or a parameter list paren.
            || (trimmed.contains("=>")
                && (trimmed.ends_with("=>")
                    || trimmed.contains(") =>")
                    || trimmed.contains(") => {")));
        let has_open_brace = trimmed.contains('{') || trimmed.contains(':');

        if is_function && has_open_brace {
            let start = i;
            let mut depth = 0;
            let mut found_body = false;

            for (j, l) in lines
                .iter()
                .enumerate()
                .take(lines.len().min(start + 200))
                .skip(start)
            {
                depth += l.bytes().filter(|&b| b == b'{').count();
                depth = depth.saturating_sub(l.bytes().filter(|&b| b == b'}').count());
                if depth == 0 && j > start {
                    let func_lines = j - start;
                    if func_lines > 60 {
                        long_funcs.push((start + 1, func_lines));
                    }
                    i = j;
                    found_body = true;
                    break;
                }
            }
            if !found_body {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    if let Some(&(line, count)) = long_funcs.first() {
        Some(Finding {
            detector: "boilerplate".to_string(),
            severity: "warning",
            file: path.to_string(),
            line,
            column: 0,
            message: format!(
                "Very long function/method detected ({} lines) — AI code tends to write overly long functions.",
                count
            ),
            suggestion: Some(
                "Break this function into smaller, focused functions. Aim for <30 lines per function."
                    .to_string(),
            ),
            evidence: None,
            codemod: None,
        })
    } else {
        None
    }
}

fn check_repeated_code(path: &str, lines: &[&str]) -> Option<Finding> {
    // O(n) detection via HashMap of 3-line block fingerprints instead of O(n²) sliding window.
    // Uses a hash of the text content to avoid borrowing slices into the source.
    let mut blocks: std::collections::HashMap<u64, Vec<usize>> =
        std::collections::HashMap::new();
    use std::hash::{Hash, Hasher};

    let mut i = 0;
    while i + 3 < lines.len() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for line in &lines[i..i + 3] {
            line.hash(&mut hasher);
        }
        blocks.entry(hasher.finish()).or_default().push(i);
        i += 1;
    }

    let repeats: usize = blocks
        .values()
        .filter(|v| v.len() > 1)
        .map(|v| v.len())
        .sum();

    if repeats > 3 {
        Some(Finding {
            detector: "boilerplate".to_string(),
            severity: "warning",
            file: path.to_string(),
            line: 0,
            column: 0,
            message: format!(
                "Repeated code blocks found ({} instances) — AI often generates repetitive code.",
                repeats
            ),
            suggestion: Some("Extract repeated blocks into reusable functions.".to_string()),
            evidence: None,
            codemod: None,
        })
    } else {
        None
    }
}

fn count_comment_lines(lines: &[&str]) -> usize {
    lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            t.starts_with("//")
                || t.starts_with('#')
                || t.starts_with("/*")
                || t.starts_with('*')
                || t.starts_with("///")
                || t.starts_with("/**")
                || t.starts_with("<!--")
        })
        .count()
}

fn check_boilerplate_getters_setters(path: &str, content: &str) -> Option<Finding> {
    let (getter_count, setter_count) = content.lines().fold((0, 0), |(g, s), line| {
        (
            g + if line.contains("get_") { 1 } else { 0 },
            s + if line.contains("set_") { 1 } else { 0 },
        )
    });

    if getter_count > 5 || setter_count > 5 {
        Some(Finding {
            detector: "boilerplate".to_string(),
            severity: "info",
            file: path.to_string(),
            line: 0,
            column: 0,
            message: format!(
                "{} getters/setters detected — AI tends to generate unnecessary accessor methods.",
                getter_count + setter_count
            ),
            suggestion: Some(
                "Consider using public fields or a more concise data model instead of getters/setters."
                    .to_string(),
            ),
            evidence: None,
            codemod: None,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_over_engineering_single_trait() {
        let content = "pub trait MyTrait { fn foo(&self); }\nstruct X;\nimpl MyTrait for X { fn foo(&self) {} }";
        let file = ParsedFile {
            path: "test.rs".to_string(),
            language: "rust".to_string(),
            raw_content: content.to_string(),
            lines: content
                .lines()
                .enumerate()
                .map(|(i, l)| crate::parser::SourceLine {
                    number: i + 1,
                    content: l.to_string(),
                })
                .collect(),
            imports: vec![],
        };
        let findings = detect_over_engineering(&[file]);
        assert!(!findings.is_empty(), "should detect single-impl trait");
    }

    #[test]
    fn test_detect_boilerplate_long_function() {
        let mut lines = vec![];
        lines.push("fn foo() {".to_string());
        for i in 0..70 {
            lines.push(format!("    let x_{} = {};", i, i));
        }
        lines.push("}".to_string());
        let content = lines.join("\n");
        let file = ParsedFile {
            path: "test.rs".to_string(),
            language: "rust".to_string(),
            raw_content: content,
            lines: lines
                .iter()
                .enumerate()
                .map(|(i, l)| crate::parser::SourceLine {
                    number: i + 1,
                    content: l.to_string(),
                })
                .collect(),
            imports: vec![],
        };
        let findings = detect_boilerplate(&[file]);
        assert!(!findings.is_empty(), "should detect long function");
    }

    #[test]
    fn test_clean_code_no_style_findings() {
        let content = "fn add(a: i32, b: i32) -> i32 { a + b }";
        let file = ParsedFile {
            path: "test.rs".to_string(),
            language: "rust".to_string(),
            raw_content: content.to_string(),
            lines: vec![crate::parser::SourceLine {
                number: 1,
                content: content.to_string(),
            }],
            imports: vec![],
        };
        let findings_o = detect_over_engineering(std::slice::from_ref(&file));
        let findings_b = detect_boilerplate(&[file]);
        assert!(
            findings_o.is_empty(),
            "small clean fn should not trigger over-engineering"
        );
        assert!(
            findings_b.is_empty(),
            "small clean fn should not trigger boilerplate"
        );
    }
}
