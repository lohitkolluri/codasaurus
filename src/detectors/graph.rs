use crate::detectors::Finding;
use crate::graph::CodeGraph;
use crate::parser::ParsedFile;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex;

/// Global mutable code graph populated during parsing.
/// Each `run_all()` call rebuilds it from the parsed files.
static CODE_GRAPH: LazyLock<Mutex<CodeGraph>> = LazyLock::new(|| Mutex::new(CodeGraph::new()));

pub fn populate(files: &[ParsedFile]) {
    let mut graph = match CODE_GRAPH.lock() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Warning: code graph mutex poisoned, resetting");
            e.into_inner()
        }
    };
    *graph = CodeGraph::new();

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

    // Inter-file dependency detection — flattened from O(F²) to O(F).
    // Pre-collect ALL import names + their source files once,
    // then scan each file's words against the flat list.
    // Avoids the original nested file_a × file_b loop (F² × L × W × I_per_file → F × L × W × I_total).
    let mut all_imports: Vec<(&str, &str)> = Vec::new(); // (import_name, file_path)
    for file in files {
        for import in &file.imports {
            all_imports.push((import.name.as_str(), file.path.as_str()));
        }
    }

    // Deduplicate to avoid redundant edge processing
    let mut seen_edges: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    // Build inverted index: word → [(import_name, file_b_path)]
    // Avoids O(L × W × I) nested loop — each word does O(1) index lookup
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
                            let from_sym = format!("{}::{}", a_path, word);
                            let to_sym = format!("{}::{}", file_b_path, import_name);
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
}

/// Lock the global code graph for read-only access.
/// Used by `codasaurus verify` to share the same graph the detectors built.
pub fn lock_graph() -> Result<std::sync::MutexGuard<'static, CodeGraph>, String> {
    CODE_GRAPH
        .lock()
        .map_err(|e| format!("code graph mutex poisoned: {}", e))
}

pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    populate(parsed_files);

    let graph = match CODE_GRAPH.lock() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Warning: code graph mutex poisoned, resetting");
            e.into_inner()
        }
    };

    for (file_path, symbols) in &graph.file_to_nodes {
        for sym in symbols {
            let affected = graph.blast_radius(sym, 3);
            if affected.len() > 5 {
                findings.push(Finding {
                    detector: "blast-radius".to_string(),
                    severity: "info",
                    file: file_path.to_string(),
                    line: 0,
                    column: 0,
                    message: format!(
                        "Symbol `{}` appears in the dependency path of {} other symbols. \
                         Changes here may have wide-reaching effects.",
                        sym,
                        affected.len()
                    ),
                    suggestion: Some(
                        "Consider the impact of changes to this symbol across the codebase. \
                         Add tests for downstream consumers."
                            .to_string(),
                    ),
                    evidence: None,
                    codemod: None,
                });
            }
        }
    }

    findings
}
