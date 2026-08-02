//! Tree-sitter symbol + edge extraction for the whole-repo index.
//!
//! Parses one source file and yields definitions (symbols) plus
//! CALLS / IMPORTS / EXTENDS / DEFINES edges. Unresolvable names are kept as
//! plain text — the graph is best-effort and query time does the matching.

use tree_sitter::{Language, Node, Parser};

pub const SYMBOL_FUNCTION: &str = "function";
pub const SYMBOL_CLASS: &str = "class";
pub const SYMBOL_METHOD: &str = "method";
pub const SYMBOL_CONST: &str = "const";
pub const SYMBOL_IMPORT: &str = "import";

pub const EDGE_CALLS: &str = "CALLS";
pub const EDGE_IMPORTS: &str = "IMPORTS";
pub const EDGE_EXTENDS: &str = "EXTENDS";
pub const EDGE_DEFINES: &str = "DEFINES";

#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub name: String,
    pub kind: String,
    pub signature: Option<String>,
    pub line: i64,
}

#[derive(Debug, Clone)]
pub struct ExtractedEdge {
    pub from_symbol: String,
    pub to_symbol: String,
    pub edge_kind: String,
}

#[derive(Debug, Clone)]
pub struct FileIndex {
    pub file_path: String,
    pub symbols: Vec<ExtractedSymbol>,
    pub edges: Vec<ExtractedEdge>,
}

static RUST: std::sync::LazyLock<Language> =
    std::sync::LazyLock::new(|| Language::new(tree_sitter_rust::LANGUAGE));
static GO: std::sync::LazyLock<Language> =
    std::sync::LazyLock::new(|| Language::new(tree_sitter_go::LANGUAGE));
static PYTHON: std::sync::LazyLock<Language> =
    std::sync::LazyLock::new(|| Language::new(tree_sitter_python::LANGUAGE));
static JAVASCRIPT: std::sync::LazyLock<Language> =
    std::sync::LazyLock::new(|| Language::new(tree_sitter_javascript::LANGUAGE));
static TYPESCRIPT: std::sync::LazyLock<Language> =
    std::sync::LazyLock::new(|| Language::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT));

fn language_for_path(path: &str) -> Option<&'static Language> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".rs") {
        Some(&RUST)
    } else if lower.ends_with(".go") {
        Some(&GO)
    } else if lower.ends_with(".py") {
        Some(&PYTHON)
    } else if lower.ends_with(".js") || lower.ends_with(".jsx") || lower.ends_with(".mjs") {
        Some(&JAVASCRIPT)
    } else if lower.ends_with(".ts") || lower.ends_with(".tsx") {
        Some(&TYPESCRIPT)
    } else {
        None
    }
}

pub fn language_name(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".rs") {
        Some("rust")
    } else if lower.ends_with(".go") {
        Some("go")
    } else if lower.ends_with(".py") {
        Some("python")
    } else if lower.ends_with(".js") || lower.ends_with(".jsx") || lower.ends_with(".mjs") {
        Some("javascript")
    } else if lower.ends_with(".ts") || lower.ends_with(".tsx") {
        Some("typescript")
    } else {
        None
    }
}

/// Parse a source file into symbols + edges. `None` when the language is not
/// indexed or the tree fails to build (e.g. binary bytes).
pub fn extract_file(file_path: &str, content: &str) -> Option<FileIndex> {
    let language = language_for_path(file_path)?;
    let mut parser = Parser::new();
    parser.set_language(language).ok()?;

    let tree = parser.parse(content, None)?;
    let mut collector = Collector {
        file_path,
        content,
        symbols: Vec::new(),
        edges: Vec::new(),
        scope: Vec::new(),
    };
    collector.walk(tree.root_node());
    Some(FileIndex {
        file_path: file_path.to_string(),
        symbols: collector.symbols,
        edges: collector.edges,
    })
}

struct Collector<'a> {
    file_path: &'a str,
    content: &'a str,
    symbols: Vec<ExtractedSymbol>,
    edges: Vec<ExtractedEdge>,
    /// Enclosing definition names, innermost last. Methods/consts get
    /// `Parent::name` qualification from the top of the stack.
    scope: Vec<String>,
}

impl<'a> Collector<'a> {
    fn text(&self, node: Node) -> Option<String> {
        node.utf8_text(self.content.as_bytes())
            .ok()
            .map(|s| s.to_string())
    }

    fn node_name(&self, node: Node) -> Option<String> {
        node.child_by_field_name("name").and_then(|n| self.text(n))
    }

    fn enclosing(&self) -> Option<&str> {
        self.scope.last().map(|s| s.as_str())
    }

    /// Record a symbol (+ DEFINES edge) and return its (possibly qualified) name.
    fn add_symbol(&mut self, name: &str, kind: &str, node: Node) -> String {
        let qualified = match (self.enclosing(), kind) {
            (Some(parent), SYMBOL_METHOD) => format!("{parent}::{name}"),
            _ => name.to_string(),
        };
        let signature = self
            .text(node)
            .map(|t| t.lines().next().unwrap_or_default().trim().to_string())
            .map(|t| t.chars().take(160).collect());
        self.symbols.push(ExtractedSymbol {
            name: qualified.clone(),
            kind: kind.to_string(),
            signature,
            line: node.start_position().row as i64 + 1,
        });
        self.edges.push(ExtractedEdge {
            from_symbol: self.file_path.to_string(),
            to_symbol: qualified.clone(),
            edge_kind: EDGE_DEFINES.to_string(),
        });
        qualified
    }

    fn walk(&mut self, node: Node) {
        let kind = node.kind();
        let mut pushed = false;

        match kind {
            // --- Rust ---
            "function_item" => {
                if let Some(name) = self.node_name(node) {
                    let kind = if self.enclosing().is_some() {
                        SYMBOL_METHOD
                    } else {
                        SYMBOL_FUNCTION
                    };
                    let qualified = self.add_symbol(&name, kind, node);
                    self.scope.push(qualified);
                    pushed = true;
                }
            }
            "struct_item" | "enum_item" | "trait_item" | "union_item" => {
                if let Some(name) = self.node_name(node) {
                    let qualified = self.add_symbol(&name, SYMBOL_CLASS, node);
                    self.scope.push(qualified);
                    pushed = true;
                }
            }
            "impl_item" => {
                let name = node.child_by_field_name("type").and_then(|n| self.text(n));
                if let Some(name) = name {
                    let qualified = self.add_symbol(&name, SYMBOL_CLASS, node);
                    self.scope.push(qualified);
                    pushed = true;
                }
            }
            "const_item" | "static_item" => {
                if let Some(name) = self.node_name(node) {
                    self.add_symbol(&name, SYMBOL_CONST, node);
                }
            }
            "use_declaration" => {
                if let Some(text) = self.text(node) {
                    let clean = text.trim_start_matches("use").trim().trim_end_matches(';');
                    self.push_import(clean.to_string(), node);
                }
            }
            // --- Go ---
            "function_declaration" => {
                if let Some(name) = self.node_name(node) {
                    let qualified = self.add_symbol(&name, SYMBOL_FUNCTION, node);
                    self.scope.push(qualified);
                    pushed = true;
                }
            }
            "method_declaration" => {
                if let Some(name) = self.node_name(node) {
                    let qualified = self.add_symbol(&name, SYMBOL_METHOD, node);
                    self.scope.push(qualified);
                    pushed = true;
                }
            }
            "type_declaration" => {
                if let Some(spec) = node.child_by_field_name("type") {
                    if let Some(name) = self.node_name(spec) {
                        let qualified = self.add_symbol(&name, SYMBOL_CLASS, node);
                        self.scope.push(qualified);
                        pushed = true;
                    }
                }
            }
            "const_declaration" | "var_declaration" => {
                if let Some(name) = self.node_name(node) {
                    self.add_symbol(&name, SYMBOL_CONST, node);
                }
            }
            "import_declaration" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "import_spec" {
                        if let Some(path) = child.child_by_field_name("path") {
                            if let Some(clean) = self
                                .text(path)
                                .map(|t| t.trim_matches('"').to_string())
                                .filter(|t| !t.is_empty())
                            {
                                self.push_import(clean, node);
                            }
                        }
                    }
                }
            }
            // --- Python ---
            "function_definition" => {
                if let Some(name) = self.node_name(node) {
                    let kind = if self.enclosing().is_some() {
                        SYMBOL_METHOD
                    } else {
                        SYMBOL_FUNCTION
                    };
                    let qualified = self.add_symbol(&name, kind, node);
                    self.scope.push(qualified);
                    pushed = true;
                }
            }
            "class_definition" => {
                if let Some(name) = self.node_name(node) {
                    let qualified = self.add_symbol(&name, SYMBOL_CLASS, node);
                    if let Some(superclasses) = node.child_by_field_name("superclasses") {
                        for child in superclasses.named_children(&mut superclasses.walk()) {
                            if let Some(base) = self.text(child) {
                                self.edges.push(ExtractedEdge {
                                    from_symbol: name.clone(),
                                    to_symbol: base,
                                    edge_kind: EDGE_EXTENDS.to_string(),
                                });
                            }
                        }
                    }
                    self.scope.push(qualified);
                    pushed = true;
                }
            }
            "import_statement" | "import_from_statement" => {
                if let Some(text) = self.text(node) {
                    let clean = text.replace("import", "").trim().to_string();
                    self.push_import(clean, node);
                }
            }
            // --- JavaScript / TypeScript ---
            "class_declaration" | "interface_declaration" => {
                if let Some(name) = self.node_name(node) {
                    let qualified = self.add_symbol(&name, SYMBOL_CLASS, node);
                    if let Some(heritage) = node.child_by_field_name("heritage") {
                        for child in heritage.named_children(&mut heritage.walk()) {
                            if let Some(base) = self.text(child) {
                                self.edges.push(ExtractedEdge {
                                    from_symbol: name.clone(),
                                    to_symbol: base,
                                    edge_kind: EDGE_EXTENDS.to_string(),
                                });
                            }
                        }
                    }
                    self.scope.push(qualified);
                    pushed = true;
                }
            }
            "method_definition" => {
                if let Some(name) = self.node_name(node) {
                    let qualified = self.add_symbol(&name, SYMBOL_METHOD, node);
                    self.scope.push(qualified);
                    pushed = true;
                }
            }
            "variable_declarator" => {
                let is_const = node
                    .parent()
                    .and_then(|p| p.child(0))
                    .and_then(|k| self.text(k))
                    .map(|t| t == "const")
                    .unwrap_or(false);
                if is_const {
                    if let Some(name) = self.node_name(node) {
                        self.add_symbol(&name, SYMBOL_CONST, node);
                    }
                }
            }
            _ => {}
        }

        // CALLS edges: name the callee from the call's function field.
        if matches!(kind, "call_expression" | "call") {
            if let Some(callee) = node.child_by_field_name("function") {
                let callee_name = if callee.kind() == "member_expression" {
                    callee
                        .child_by_field_name("property")
                        .and_then(|n| self.text(n))
                } else if callee.kind() == "field_expression" {
                    callee
                        .child_by_field_name("field")
                        .and_then(|n| self.text(n))
                } else {
                    callee
                        .child_by_field_name("name")
                        .and_then(|n| self.text(n))
                        .or_else(|| self.text(callee))
                };
                if let Some(target) = callee_name {
                    if let Some(caller) = self.enclosing() {
                        self.edges.push(ExtractedEdge {
                            from_symbol: caller.to_string(),
                            to_symbol: target,
                            edge_kind: EDGE_CALLS.to_string(),
                        });
                    }
                }
            }
        }

        // Recurse, then pop the scope entry this node pushed (if any).
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk(child);
        }
        if pushed {
            self.scope.pop();
        }
    }

    fn push_import(&mut self, name: String, node: Node) {
        self.symbols.push(ExtractedSymbol {
            name: name.clone(),
            kind: SYMBOL_IMPORT.to_string(),
            signature: None,
            line: node.start_position().row as i64 + 1,
        });
        self.edges.push(ExtractedEdge {
            from_symbol: self.file_path.to_string(),
            to_symbol: name,
            edge_kind: EDGE_IMPORTS.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_extracts_functions_and_calls() {
        let src = r#"
fn greet(name: &str) -> String {
    format!("hi {}", name)
}

fn main() {
    let g = greet("x");
}
"#;
        let idx = extract_file("src/main.rs", src).unwrap();
        let names: Vec<&str> = idx.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"greet"), "{names:?}");
        assert!(names.contains(&"main"), "{names:?}");
        let calls = idx
            .edges
            .iter()
            .filter(|e| e.edge_kind == EDGE_CALLS)
            .collect::<Vec<_>>();
        assert!(
            calls
                .iter()
                .any(|e| e.from_symbol == "main" && e.to_symbol == "greet"),
            "{calls:?}"
        );
    }

    #[test]
    fn python_extracts_class_and_extends() {
        let src = "import os\nclass Animal:\n    pass\nclass Dog(Animal):\n    pass\n";
        let idx = extract_file("app.py", src).unwrap();
        let names: Vec<&str> = idx.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Animal"), "{names:?}");
        assert!(names.contains(&"Dog"), "{names:?}");
        assert!(idx.edges.iter().any(|e| {
            e.edge_kind == EDGE_EXTENDS && e.from_symbol == "Dog" && e.to_symbol == "Animal"
        }));
    }

    #[test]
    fn go_extracts_imports() {
        let src = "package main\nimport \"fmt\"\nfunc main() { fmt.Println(\"hi\") }\n";
        let idx = extract_file("main.go", src).unwrap();
        assert!(idx
            .edges
            .iter()
            .any(|e| e.edge_kind == EDGE_IMPORTS && e.to_symbol == "fmt"));
        assert!(idx.symbols.iter().any(|s| s.name == "main"));
    }

    #[test]
    fn javascript_extracts_methods() {
        let src = "class User {\n  async load() { return fetch('/u'); }\n}\nconst MAX = 5;\n";
        let idx = extract_file("user.js", src).unwrap();
        let names: Vec<&str> = idx.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"User"), "{names:?}");
        assert!(names.contains(&"User::load"), "{names:?}");
        assert!(names.contains(&"MAX"), "{names:?}");
    }

    #[test]
    fn rust_impl_methods_qualified() {
        let src = "struct User {}\nimpl User {\n    fn load() -> u8 { 1 }\n}\n";
        let idx = extract_file("src/user.rs", src).unwrap();
        let names: Vec<&str> = idx.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"User"), "{names:?}");
        assert!(names.contains(&"User::load"), "{names:?}");
    }

    #[test]
    fn python_class_methods_qualified() {
        let src = "class A:\n    def go(self):\n        pass\n";
        let idx = extract_file("a.py", src).unwrap();
        let names: Vec<&str> = idx.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"A"), "{names:?}");
        assert!(names.contains(&"A::go"), "{names:?}");
    }

    #[test]
    fn javascript_method_calls_use_field_name() {
        let src = "class A {\n  go() { return this.b(); }\n}\n";
        let idx = extract_file("a.js", src).unwrap();
        let calls: Vec<String> = idx
            .edges
            .iter()
            .filter(|e| e.edge_kind == EDGE_CALLS)
            .map(|e| e.to_symbol.clone())
            .collect();
        assert!(calls.contains(&"b".to_string()), "{calls:?}");
        assert!(idx.edges.iter().any(|e| {
            e.edge_kind == EDGE_CALLS && e.from_symbol == "A::go" && e.to_symbol == "b"
        }));
    }

    #[test]
    fn unsupported_extension_returns_none() {
        assert!(extract_file("data.csv", "a,b\n").is_none());
    }
}
