use crate::detectors::Finding;
use crate::graph::CodeGraph;
use crate::parser::ParsedFile;
use std::collections::HashMap;

/// Cap files scanned when building word→import edges (O(files × lines × words)).
const MAX_WORD_EDGE_FILES: usize = 64;
/// Cap BFS blast-radius checks per review.
const MAX_BFS_SYMBOLS: usize = 40;
/// Stop expanding BFS once we already know the radius is "large".
const BLAST_EARLY_STOP: usize = 24;
/// Skip full BFS when direct out-degree already exceeds this.
const HIGH_DEGREE_SKIP: usize = 16;

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
    for file in files.iter().take(MAX_WORD_EDGE_FILES) {
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

    for file_a in files.iter().take(MAX_WORD_EDGE_FILES) {
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

/// Prefer file-local symbols (path::name) over bare package imports for BFS.
fn is_file_local_symbol(sym: &str) -> bool {
    sym.contains('/') || sym.contains('\\') || sym.contains('.')
}

pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    if parsed_files.is_empty() {
        return findings;
    }
    let graph = build_graph(parsed_files);

    // Input files are the changed/scanned set for this review — only BFS those.
    let changed: std::collections::HashSet<&str> =
        parsed_files.iter().map(|f| f.path.as_str()).collect();

    let mut bfs_done = 0usize;
    'outer: for (file_path, symbols) in &graph.file_to_nodes {
        if !changed.contains(file_path.as_str()) {
            continue;
        }
        for sym in symbols {
            if bfs_done >= MAX_BFS_SYMBOLS {
                break 'outer;
            }
            // Package-name import nodes fan out widely; skip unless file-local.
            if !is_file_local_symbol(sym) {
                continue;
            }

            let degree = graph.out_degree(sym);
            if degree > HIGH_DEGREE_SKIP {
                // Already large enough to surface without a full BFS.
                findings.push(Finding {
                    detector: "graph".to_string(),
                    severity: "info",
                    file: file_path.clone(),
                    line: 0,
                    column: 0,
                    message: format!(
                        "Symbol `{sym}` has a large blast radius ({degree}+ direct dependents)"
                    ),
                    suggestion: Some(
                        "Review callers before changing this symbol; consider feature flags."
                            .into(),
                    ),
                    evidence: None,
                    codemod: None,
                    confidence: None,
                    judge_rationale: None,
                });
                bfs_done += 1;
                continue;
            }

            let affected = graph.blast_radius_capped(sym, 3, BLAST_EARLY_STOP);
            bfs_done += 1;
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
                    confidence: None,
                    judge_rationale: None,
                });
            }
        }
    }

    findings
}
