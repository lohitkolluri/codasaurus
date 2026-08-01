use crate::detectors::Finding;
use crate::graph::CodeGraph;
use crate::parser::ParsedFile;
use std::collections::HashMap;

/// Build a per-review code graph (never share across concurrent reviews).
fn build_graph(files: &[ParsedFile]) -> CodeGraph {
    let mut graph = CodeGraph::new();

    for file in files {
        let path = &file.path;

        for import in &file.imports {
            let package = crate::detectors::extract_package_name(&import.name);
            if let Some(pkg) = package {
                if !pkg.starts_with('.') && !pkg.starts_with('/') {
                    graph.add_symbol(&pkg, path, crate::graph::SymbolKind::Import);
                    graph.add_symbol(
                        &format!("{}::{}", path, import.name),
                        path,
                        crate::graph::SymbolKind::Variable,
                    );
                    graph.add_edge(
                        &format!("{}::{}", path, import.name),
                        &pkg,
                        crate::graph::EdgeKind::Imports,
                    );
                }
            }
        }
    }

    let mut all_imports: Vec<(&str, &str)> = Vec::new();
    for file in files {
        for import in &file.imports {
            all_imports.push((import.name.as_str(), file.path.as_str()));
        }
    }

    let mut seen_edges: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    let mut word_to_imports: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for &(import_name, file_b_path) in &all_imports {
        for word in import_name.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if word.len() >= 3 {
                word_to_imports
                    .entry(word)
                    .or_default()
                    .push((import_name, file_b_path));
            }
        }
    }

    for file_a in files {
        let a_path = file_a.path.as_str();
        for line in &file_a.lines {
            let trimmed = line.content.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }
            for word in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if word.len() < 3 {
                    continue;
                }
                if let Some(matches) = word_to_imports.get(word) {
                    for &(import_name, file_b_path) in matches {
                        if file_b_path == a_path {
                            continue;
                        }
                        if word == import_name {
                            let from_sym = format!("{a_path}::{word}");
                            let to_sym = format!("{file_b_path}::{import_name}");
                            let edge_key = (from_sym.clone(), to_sym.clone());
                            if seen_edges.insert(edge_key) {
                                graph.add_symbol(
                                    &from_sym,
                                    a_path,
                                    crate::graph::SymbolKind::Variable,
                                );
                                graph.add_symbol(
                                    &to_sym,
                                    file_b_path,
                                    crate::graph::SymbolKind::Variable,
                                );
                                graph.add_edge(&from_sym, &to_sym, crate::graph::EdgeKind::Calls);
                            }
                        }
                    }
                }
            }
        }
    }

    graph
}

pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let graph = build_graph(parsed_files);

    for (file_path, symbols) in &graph.file_to_nodes {
        for sym in symbols {
            let affected = graph.blast_radius(sym, 3);
            if affected.len() > 5 {
                findings.push(Finding {
                    detector: "graph".to_string(),
                    severity: "info",
                    file: file_path.clone(),
                    line: 0,
                    column: 0,
                    message: format!(
                        "Symbol `{sym}` has a large blast radius ({} dependents)",
                        affected.len()
                    ),
                    suggestion: Some(
                        "Review callers before changing this symbol; consider feature flags."
                            .into(),
                    ),
                    evidence: None,
                    codemod: None,
                });
            }
        }
    }

    findings
}
