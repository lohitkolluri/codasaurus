use crate::detectors::{Finding, Findings};
use crate::parser::ParsedFile;

/// Detect over-engineered patterns in AI-generated code
/// Looks for unnecessary abstractions, excessive nesting, over-use of patterns
pub fn detect_over_engineering(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for file in parsed_files {
        let content = &file.raw_content;
        let lines: Vec<&str> = content.lines().collect();

        // Check for unnecessary interface/trait with single implementation
        if let Some(finding) = check_single_impl_interface(&file.path, &lines) {
            findings.push(finding);
        }

        // Check for deep nesting (more than 4 levels is suspicious in AI code)
        if let Some(finding) = check_deep_nesting(&file.path, &lines) {
            findings.push(finding);
        }

        // Check for factory pattern with very few variants
        if let Some(finding) = check_unnecessary_factory(&file.path, content) {
            findings.push(finding);
        }

        // Check for excessive abstraction layers
        if let Some(finding) = check_abstraction_overload(&file.path, content) {
            findings.push(finding);
        }
    }

    findings
}

/// Detect boilerplate and unnecessarily verbose code
pub fn detect_boilerplate(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for file in parsed_files {
        let content = &file.raw_content;
        let lines: Vec<&str> = content.lines().collect();
        let line_count = lines.len();

        // Check for very long functions/methods (AI tendency: write long functions)
        if let Some(finding) = check_long_functions(&file.path, content) {
            findings.push(finding);
        }

        // Check for repeated code blocks
        if let Some(finding) = check_repeated_code(&file.path, &lines) {
            findings.push(finding);
        }

        // Check for excessive comments (AI tendency: over-comment)
        if line_count > 30 {
            let comment_ratio = count_comment_lines(&lines) as f64 / line_count as f64;
            if comment_ratio > 0.4 {
                findings.push(Finding {
                    detector: "boilerplate".to_string(),
                    severity: "warning".to_string(),
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
                });
            }
        }

        // Check for repetitive getter/setter patterns
        if let Some(finding) = check_boilerplate_getters_setters(&file.path, content) {
            findings.push(finding);
        }
    }

    findings
}

fn check_single_impl_interface(path: &str, lines: &[&str]) -> Option<Finding> {
    let content = lines.join("\n");
    let trait_count = content.matches("trait ").count();
    let impl_count = content.matches("impl ").count();

    // If there's 1 trait with 1 impl, it's suspicious
    if trait_count == 1 && impl_count == 1 {
        return Some(Finding {
            detector: "over-engineering".to_string(),
            severity: "warning".to_string(),
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
        });
    }

    None
}

fn check_deep_nesting(path: &str, lines: &[&str]) -> Option<Finding> {
    let mut max_depth = 0;
    let mut current_depth = 0;
    let mut deepest_line = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }

        // Count opening braces/brackets
        let opens = trimmed.matches('{').count() + trimmed.matches('(').count();
        let closes = trimmed.matches('}').count() + trimmed.matches(')').count();

        if opens > closes {
            current_depth += opens - closes;
        } else if closes > opens {
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
            severity: "info".to_string(),
            file: path.to_string(),
            line: deepest_line,
            column: 0,
            message: format!(
                "Deep nesting ({} levels) — AI code often creates unnecessarily nested structures.",
                max_depth
            ),
            suggestion: Some(
                "Extract nested logic into separate functions to improve readability."
                    .to_string(),
            ),
            evidence: None,
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
    let struct_count = content.matches("struct ").count();
    let class_count = content.matches("class ").count();
    let total_types = struct_count + class_count;

    // Factory with very few types is over-engineering
    if total_types <= 3 && has_factory {
        Some(Finding {
            detector: "over-engineering".to_string(),
            severity: "warning".to_string(),
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
            severity: "warning".to_string(),
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
        })
    } else {
        None
    }
}

fn check_long_functions(path: &str, content: &str) -> Option<Finding> {
    // Simple heuristic: count lines between function-like patterns
    let mut long_funcs = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Detect function/method definitions
        let is_function = trimmed.contains("fn ")
            || trimmed.contains("def ")
            || trimmed.contains("function ")
            || trimmed.contains("=>");
        let has_open_brace = trimmed.contains('{') || trimmed.contains(':');

        if is_function && has_open_brace {
            let start = i;
            let mut depth = 0;
            let mut found_body = false;

            for j in start..lines.len().min(start + 200) {
                let l = lines[j];
                depth += l.matches('{').count();
                depth = depth.saturating_sub(l.matches('}').count());
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
            severity: "warning".to_string(),
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
        })
    } else {
        None
    }
}

fn check_repeated_code(path: &str, lines: &[&str]) -> Option<Finding> {
    // Simple check: look for repeated blocks of 3+ identical lines
    let mut repeats = 0;
    let mut i = 0;

    while i + 3 < lines.len() {
        let block = &lines[i..i + 3];
        for j in (i + 3..lines.len() - 3).step_by(1) {
            if lines[j..j + 3] == *block {
                repeats += 1;
                break;
            }
        }
        i += 1;
    }

    if repeats > 3 {
        Some(Finding {
            detector: "boilerplate".to_string(),
            severity: "warning".to_string(),
            file: path.to_string(),
            line: 0,
            column: 0,
            message: format!(
                "Repeated code blocks found ({} instances) — AI often generates repetitive code.",
                repeats
            ),
            suggestion: Some("Extract repeated blocks into reusable functions.".to_string()),
            evidence: None,
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
    let getter_count = content.matches("get_").count();
    let setter_count = content.matches("set_").count();

    if getter_count > 5 || setter_count > 5 {
        Some(Finding {
            detector: "boilerplate".to_string(),
            severity: "info".to_string(),
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
        })
    } else {
        None
    }
}
