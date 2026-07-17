use anyhow::Result;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

mod blast;

#[derive(Debug, Clone)]
pub struct SymbolNode {
    pub name: String,
    pub file: String,
    pub kind: SymbolKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Function,
    Class,
    Import,
    Variable,
    Type,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EdgeKind {
    Calls,
    Imports,
    Extends,
    Contains,
}

/// Codebase dependency graph
pub struct CodeGraph {
    graph: DiGraph<SymbolNode, EdgeKind>,
    node_indices: HashMap<String, NodeIndex>,
    files: HashMap<String, Vec<NodeIndex>>,
}

impl CodeGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_indices: HashMap::new(),
            files: HashMap::new(),
        }
    }

    pub fn add_symbol(&mut self, name: &str, file: &str, kind: SymbolKind) {
        let idx = self.graph.add_node(SymbolNode {
            name: name.to_string(),
            file: file.to_string(),
            kind,
        });
        self.node_indices.insert(name.to_string(), idx);
        self.files
            .entry(file.to_string())
            .or_default()
            .push(idx);
    }

    pub fn add_edge(&mut self, from: &str, to: &str, kind: EdgeKind) {
        if let (Some(&from_idx), Some(&to_idx)) =
            (self.node_indices.get(from), self.node_indices.get(to))
        {
            self.graph.add_edge(from_idx, to_idx, kind);
        }
    }

    /// Find all symbols that could be affected by a change to `symbol`
    pub fn blast_radius(&self, symbol: &str, max_hops: usize) -> Vec<&SymbolNode> {
        let mut affected = Vec::new();
        if let Some(&start) = self.node_indices.get(symbol) {
            use petgraph::visit::Bfs;
            let mut bfs = Bfs::new(&self.graph, start);
            while let Some(nx) = bfs.next(&self.graph) {
                let node = &self.graph[nx];
                affected.push(node);
            }
        }
        affected
    }

    /// Get all files that contain symbols affected by a change
    pub fn affected_files(&self, symbol: &str, max_hops: usize) -> Vec<&str> {
        let mut files: Vec<&str> = self
            .blast_radius(symbol, max_hops)
            .iter()
            .map(|n| n.file.as_str())
            .collect();
        files.sort();
        files.dedup();
        files
    }
}
