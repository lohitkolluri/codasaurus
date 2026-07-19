use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;

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

#[derive(Debug, Clone, PartialEq)]
pub enum EdgeKind {
    Calls,
    Imports,
}

pub struct CodeGraph {
    graph: DiGraph<SymbolNode, EdgeKind>,
    pub node_indices: HashMap<String, NodeIndex>,
    pub files: HashMap<String, Vec<NodeIndex>>,
    pub file_to_nodes: HashMap<String, Vec<String>>,
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_indices: HashMap::new(),
            files: HashMap::new(),
            file_to_nodes: HashMap::new(),
        }
    }

    pub fn add_symbol(&mut self, name: &str, file: &str, kind: SymbolKind) {
        let idx = self.graph.add_node(SymbolNode {
            name: name.to_string(),
            file: file.to_string(),
            kind,
        });
        self.node_indices.insert(name.to_string(), idx);
        self.files.entry(file.to_string()).or_default().push(idx);
        self.file_to_nodes
            .entry(file.to_string())
            .or_default()
            .push(name.to_string());
    }

    pub fn add_edge(&mut self, from: &str, to: &str, kind: EdgeKind) {
        if let (Some(&from_idx), Some(&to_idx)) =
            (self.node_indices.get(from), self.node_indices.get(to))
        {
            self.graph.add_edge(from_idx, to_idx, kind);
        }
    }

    /// Find all symbols that could be affected by a change to `symbol`, up to `max_hops` away.
    pub fn blast_radius(&self, symbol: &str, max_hops: usize) -> Vec<&SymbolNode> {
        let mut affected = Vec::new();
        if let Some(&start) = self.node_indices.get(symbol) {
            let mut visited = std::collections::HashSet::new();
            let mut queue = std::collections::VecDeque::new();
            let mut depths = std::collections::HashMap::new();

            visited.insert(start);
            queue.push_back(start);
            depths.insert(start, 0);

            while let Some(nx) = queue.pop_front() {
                let d = depths[&nx];
                affected.push(&self.graph[nx]);

                if d < max_hops {
                    for neighbor in self.graph.neighbors(nx) {
                        if visited.insert(neighbor) {
                            depths.insert(neighbor, d + 1);
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }
        affected
    }

    /// Find direct callers of a symbol (reverse edges from `symbol`).
    pub fn find_callers(&self, symbol: &str) -> Vec<&SymbolNode> {
        let mut callers = Vec::new();
        if let Some(&idx) = self.node_indices.get(symbol) {
            for edge in self
                .graph
                .edges_directed(idx, petgraph::Direction::Incoming)
            {
                let source = edge.source();
                callers.push(&self.graph[source]);
            }
        }
        callers
    }

    /// Get unique files affected by changes to the given symbol set.
    pub fn affected_files(&self, symbols: &[String]) -> Vec<String> {
        let mut files: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for sym in symbols {
            let radius = self.blast_radius(sym, 3);
            for node in radius {
                files.insert(node.file.clone());
            }
        }
        files.into_iter().collect()
    }

    /// Find symbols by file path — returns all symbol names in the given file.
    pub fn symbols_in_file(&self, file: &str) -> Vec<&str> {
        self.file_to_nodes
            .get(file)
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    /// Heuristically map a code symbol name to potential test names.
    ///
    /// Conventions handled:
    /// - Rust: `my_module::tests::test_my_fn` or `my_module::tests::my_fn`
    /// - Python: `test_my_function` in `test_my_module.py`
    /// - JS/TS: `describe("myFunction")` / `it("should ...")` — maps to `myFunction`
    /// - Go: `TestMyFunction` in `my_function_test.go`
    ///
    /// Returns test name fragments that can be passed to `cargo test` or equivalents.
    pub fn symbol_to_test_names(&self, symbol: &str) -> Vec<String> {
        let mut names = Vec::new();

        // Strip path prefix: "src/main.rs::my_fn" -> "my_fn"
        let base = symbol.rsplit("::").next().unwrap_or(symbol);
        // Strip file prefix like "src/main.rs::"
        let base = base.split("::").last().unwrap_or(base);

        // Rust: my_fn -> tests::test_my_fn, tests::my_fn
        names.push(format!("tests::test_{}", base));
        names.push(format!("tests::{}", base));

        // Rust: my_fn -> test_my_fn (direct fn name match)
        names.push(format!("test_{}", base));

        // PascalCase name -> TestName (Go convention)
        if base.chars().next().map_or(false, |c| c.is_uppercase()) {
            names.push(format!("Test{}", base));
        }

        // Module-style: module::tests::test_name
        if let Some(module) = symbol
            .rsplitn(2, '/')
            .next()
            .and_then(|s| s.rsplit('.').next())
        {
            names.push(format!("{}::tests::test_{}", module, base));
        }

        names
    }
}
