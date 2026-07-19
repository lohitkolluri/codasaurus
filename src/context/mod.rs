pub mod guidelines;
pub mod rules;

use std::collections::HashMap;
use std::fmt;
use std::io::BufRead;
use std::path::Path;

/// Maximum number of entries to walk when building repo context.
/// Prevents O(n) full-tree walks on large repos from blocking every LLM review call.
const MAX_WALK_ENTRIES: usize = 10_000;

/// Directories whose names (lowercased) are skipped from traversal entirely.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "dist",
    ".git",
    ".svn",
    "__pycache__",
    ".venv",
    "venv",
    "env",
    ".tox",
    "vendor",
    ".next",
    ".nuxt",
    "out",
    "bin",
    "obj",
    "coverage",
    ".nyc_output",
    "elm-stuff",
    ".gradle",
    ".idea",
    ".vscode",
    ".terraform",
    ".serverless",
    "third_party",
    "third-party",
];

/// Dependency manifest file names we recognize (lowercased).
const DEP_FILE_NAMES: &[&str] = &[
    "package.json",
    "requirements.txt",
    "pyproject.toml",
    "setup.py",
    "setup.cfg",
    "cargo.toml",
    "go.mod",
    "gemfile",
    "gemfile.lock",
];

/// Maps a dep file name (lowered) to its registry label.
const DEP_REGISTRY_MAP: &[(&str, &str)] = &[
    ("package.json", "npm"),
    ("cargo.toml", "crates.io"),
    ("go.mod", "go"),
    ("gemfile", "rubygems"),
    ("gemfile.lock", "rubygems"),
    // everything else -> "pypi"
];

fn ext_to_lang(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some("Rust"),
        "ts" => Some("TypeScript"),
        "tsx" => Some("TSX"),
        "js" => Some("JavaScript"),
        "jsx" => Some("JSX"),
        "py" => Some("Python"),
        "go" => Some("Go"),
        "java" => Some("Java"),
        "rb" => Some("Ruby"),
        "php" => Some("PHP"),
        "c" => Some("C"),
        "h" => Some("C/C++ Header"),
        "cpp" | "cc" | "cxx" => Some("C++"),
        "cs" => Some("C#"),
        "swift" => Some("Swift"),
        "kt" | "kts" => Some("Kotlin"),
        "scala" | "sc" => Some("Scala"),
        "toml" => Some("TOML"),
        "json" => Some("JSON"),
        "yaml" | "yml" => Some("YAML"),
        "md" => Some("Markdown"),
        "css" | "scss" | "less" => Some("CSS"),
        "html" | "htm" => Some("HTML"),
        "sh" | "bash" => Some("Shell"),
        "sql" => Some("SQL"),
        "r" => Some("R"),
        "lua" => Some("Lua"),
        "zig" => Some("Zig"),
        "dart" => Some("Dart"),
        "ex" | "exs" => Some("Elixir"),
        "clj" | "cljs" | "cljc" => Some("Clojure"),
        "erl" => Some("Erlang"),
        "hs" => Some("Haskell"),
        "vue" => Some("Vue"),
        "svelte" => Some("Svelte"),
        "tf" => Some("Terraform"),
        "proto" => Some("Protobuf"),
        _ => None,
    }
}

fn file_name_to_registry(name: &str) -> Option<&'static str> {
    for (n, reg) in DEP_REGISTRY_MAP {
        if *n == name {
            return Some(reg);
        }
    }
    None
}

// Extraction functions moved to crate::dep_parser — all use through crate::dep_parser::*

/// Count newlines in a file using a buffered reader. Avoids the allocation
/// and UTF-8 validation cost of `read_to_string` + `lines().count()`.
fn count_lines(path: &Path) -> usize {
    match std::fs::File::open(path) {
        Ok(file) => {
            // 8 KB buffer — sweet spot for typical source files
            let mut reader = std::io::BufReader::with_capacity(8192, file);
            let mut count = 0usize;
            let mut buf = String::with_capacity(256);
            loop {
                match reader.read_line(&mut buf) {
                    Ok(0) => break,
                    Ok(_) => {
                        count += 1;
                        buf.clear();
                    }
                    Err(_) => return 0,
                }
            }
            count
        }
        Err(_) => 0,
    }
}

/// Read the full content of a dep file and count its lines in one pass.
/// Returns `(lines, content)` so the caller gets both without re-reading.
fn read_dep_file(path: &Path) -> Option<(usize, String)> {
    let content = std::fs::read_to_string(path).ok()?;
    // content.lines().count() correctly handles files with and without trailing newline
    let lines = if content.is_empty() {
        0
    } else {
        content.lines().count()
    };
    Some((lines, content))
}

/// Structured summary of a repository's codebase for LLM context enrichment.
#[derive(Debug, Clone)]
pub struct RepoContext {
    pub root: String,
    pub file_count: usize,
    pub total_lines: usize,
    pub languages: Vec<LanguageStat>,
    pub dependencies: HashMap<String, Vec<String>>,
    pub tree_items: Vec<String>,
    pub guidelines_section: String,
}

#[derive(Debug, Clone)]
pub struct LanguageStat {
    pub name: String,
    pub file_count: usize,
    pub line_count: usize,
}

impl fmt::Display for RepoContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "## Repository Overview")?;
        writeln!(f)?;
        writeln!(f, "**Root**: {}", self.root)?;
        writeln!(
            f,
            "**Files**: {} source files, ~{} lines across all",
            self.file_count, self.total_lines
        )?;
        writeln!(f)?;

        if !self.languages.is_empty() {
            writeln!(f, "### Languages")?;
            writeln!(f)?;
            for lang in &self.languages {
                let pct = if self.total_lines > 0 {
                    (lang.line_count as f64 / self.total_lines as f64 * 100.0) as u32
                } else {
                    0
                };
                writeln!(
                    f,
                    "- **{}**: {} files, {} lines ({}%)",
                    lang.name, lang.file_count, lang.line_count, pct
                )?;
            }
            writeln!(f)?;
        }

        if !self.dependencies.is_empty() {
            writeln!(f, "### Known Dependencies")?;
            writeln!(f)?;
            for (registry, pkgs) in &self.dependencies {
                if pkgs.is_empty() {
                    continue;
                }
                writeln!(
                    f,
                    "**{}** ({} packages): {}",
                    registry,
                    pkgs.len(),
                    pkgs.join(", ")
                )?;
            }
            writeln!(f)?;
        }

        if !self.tree_items.is_empty() {
            writeln!(f, "### Source Tree")?;
            writeln!(f)?;
            for item in &self.tree_items {
                writeln!(f, "{}", item)?;
            }
        }

        if !self.guidelines_section.is_empty() {
            writeln!(f)?;
            write!(f, "{}", self.guidelines_section)?;
        }

        Ok(())
    }
}

/// Walk the repository at `root` and build a structured context summary.
///
/// Does a single filesystem pass: accumulates file stats, dependency
/// declarations, and a compact directory tree simultaneously.
///
/// `guidelines_override` optionally specifies a path to contribution guidelines
/// (from config or env var), which overrides auto-discovery.
pub fn build_repo_context(root: &str, guidelines_override: Option<&str>) -> Option<RepoContext> {
    let root_path = Path::new(root);
    if !root_path.is_dir() {
        return None;
    }

    let mut lang_map: HashMap<String, (usize, usize)> = HashMap::new();
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    let mut file_count = 0usize;
    let mut total_lines = 0usize;

    // Tree structure accumulated during walk (avoids a second read_dir pass)
    let mut root_dirs: Vec<String> = Vec::new();
    let mut root_files: Vec<String> = Vec::new();
    let mut sub_entries: HashMap<String, Vec<String>> = HashMap::new();

    let walker = walkdir::WalkDir::new(root_path)
        .follow_links(false)
        .max_open(256)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = match e.file_name().to_str() {
                Some(n) => n,
                None => return false,
            };
            if e.file_type().is_dir() {
                // Skip hidden dirs
                if name.starts_with('.') {
                    return false;
                }
                // Skip known generated dirs
                !SKIP_DIRS.contains(&name)
            } else {
                true
            }
        })
        .filter_map(|e| {
            if let Err(err) = &e {
                eprintln!("Warning: error reading directory entry: {}", err);
            }
            e.ok()
        });

    for entry in walker.into_iter().take(MAX_WALK_ENTRIES) {
        let depth = entry.depth();
        let path = entry.path();

        // Accumulate tree structure
        if depth == 1 {
            let fname = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if entry.file_type().is_dir() {
                root_dirs.push(fname);
            } else {
                root_files.push(fname);
            }
        } else if depth == 2 {
            let parent = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map(|n| n.to_string());
            let fname = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if let Some(p) = parent {
                let prefix = if entry.file_type().is_dir() {
                    format!("  📁 {}/", fname)
                } else {
                    format!("  📄 {}", fname)
                };
                sub_entries.entry(p).or_default().push(prefix);
            }
        }

        if !entry.file_type().is_file() {
            continue;
        }

        file_count += 1;
        let file_name_lower = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        // Is this a known dependency manifest file?
        if DEP_FILE_NAMES.contains(&file_name_lower.as_str()) {
            if let Some((lines, content)) = read_dep_file(path) {
                total_lines += lines;
                let pkgs = match file_name_lower.as_str() {
                    "package.json" => crate::dep_parser::extract_npm_deps(&content),
                    "requirements.txt" => crate::dep_parser::extract_requirements_deps(&content),
                    "pyproject.toml" => crate::dep_parser::extract_pyproject_deps(&content),
                    "setup.py" | "setup.cfg" => {
                        crate::dep_parser::extract_requirements_deps(&content)
                    }
                    "cargo.toml" => crate::dep_parser::extract_cargo_deps(&content),
                    "go.mod" => crate::dep_parser::extract_go_mod_deps(&content),
                    _ => Vec::new(),
                };
                if !pkgs.is_empty() {
                    let registry = file_name_to_registry(&file_name_lower).unwrap_or("pypi");
                    deps.entry(registry.to_string()).or_default().extend(pkgs);
                }
                // Track language for dep files too
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if let Some(lang) = ext_to_lang(ext) {
                        let lang_entry = lang_map.entry(lang.to_string()).or_insert((0, 0));
                        lang_entry.0 += 1;
                        lang_entry.1 += lines;
                    }
                }
                continue; // already handled line counting + lang tracking
            }
        }

        // Non-dep file: count lines via buffered reader (no full content load)
        let lines = count_lines(path);
        total_lines += lines;

        // Track language stats
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if let Some(lang) = ext_to_lang(ext) {
                let lang_entry = lang_map.entry(lang.to_string()).or_insert((0, 0));
                lang_entry.0 += 1;
                lang_entry.1 += lines;
            }
        }
    }

    // Build language stats (sorted by line count, descending), capped at 8
    let mut languages: Vec<LanguageStat> = lang_map
        .into_iter()
        .map(|(name, (fc, lc))| LanguageStat {
            name,
            file_count: fc,
            line_count: lc,
        })
        .collect();
    languages.sort_unstable_by_key(|b| std::cmp::Reverse(b.line_count));
    languages.truncate(8);

    // Dedup and sort dependency entries once
    for pkgs in deps.values_mut() {
        pkgs.sort_unstable();
        pkgs.dedup();
    }

    // Build the compact tree from accumulated data
    let tree_items = build_tree_from_accum(root_dirs, root_files, sub_entries);

    // Discover contribution guidelines (auto or override)
    let guideline_files = guidelines::find_guidelines(root_path, guidelines_override);
    let guidelines_section = guidelines::format_guidelines_section(&guideline_files);

    Some(RepoContext {
        root: root.to_string(),
        file_count,
        total_lines,
        languages,
        dependencies: deps,
        tree_items,
        guidelines_section,
    })
}

fn build_tree_from_accum(
    mut root_dirs: Vec<String>,
    mut root_files: Vec<String>,
    mut sub_entries: HashMap<String, Vec<String>>,
) -> Vec<String> {
    root_dirs.sort();
    root_files.sort();

    let mut result = Vec::with_capacity(root_dirs.len() + root_files.len() + 15);

    // Directories first, then files
    for dir in &root_dirs {
        result.push(format!("📁 {}/", dir));
    }
    for file in &root_files {
        result.push(format!("📄 {}", file));
    }

    // Sub-items for first 5 directories, if we have room
    if result.len() < 25 {
        for dir in root_dirs.iter().take(5) {
            if let Some(mut kids) = sub_entries.remove(dir.as_str()) {
                kids.sort();
                if kids.len() > 10 {
                    let remaining = kids.len() - 10;
                    kids.truncate(10);
                    kids.push(format!("  ... and {} more items", remaining));
                }
                result.extend(kids);
            }
        }
    }

    // Cap at 30
    if result.len() > 30 {
        let remaining = result.len() - 30;
        result.truncate(30);
        result.push(format!("... and {} more entries", remaining));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_repo_context_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = build_repo_context(dir.path().to_str().unwrap(), None);
        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert_eq!(ctx.file_count, 0);
        assert_eq!(ctx.total_lines, 0);
    }

    #[test]
    fn test_build_repo_context_with_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("lib.ts"), "export const x = 1;\n").unwrap();
        std::fs::write(root.join("README.md"), "# Project\n").unwrap();

        let ctx = build_repo_context(root.to_str().unwrap(), None).unwrap();
        assert_eq!(ctx.file_count, 3);
        assert!(ctx.total_lines >= 3);

        let langs: Vec<&str> = ctx.languages.iter().map(|l| l.name.as_str()).collect();
        assert!(langs.contains(&"Rust"));
        assert!(langs.contains(&"TypeScript"));
    }

    #[test]
    fn test_build_repo_context_bad_path() {
        assert!(build_repo_context("/nonexistent/path", None).is_none());
    }

    #[test]
    fn test_display_never_panics() {
        let ctx = RepoContext {
            root: "/test".to_string(),
            file_count: 0,
            total_lines: 0,
            languages: vec![],
            dependencies: HashMap::new(),
            tree_items: vec![],
            guidelines_section: String::new(),
        };
        let output = ctx.to_string();
        assert!(output.contains("Repository Overview"));
    }

    #[test]
    fn test_skip_hidden_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join(".hidden/secret.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("visible.rs"), "fn hello() {}\n").unwrap();

        let ctx = build_repo_context(root.to_str().unwrap(), None).unwrap();
        assert_eq!(ctx.file_count, 1);
    }

    #[test]
    fn test_count_lines_empty() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("empty.rs");
        std::fs::write(&f, "").unwrap();
        assert_eq!(count_lines(&f), 0);
    }

    #[test]
    fn test_count_lines_no_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.rs");
        std::fs::write(&f, "line1\nline2").unwrap();
        assert_eq!(count_lines(&f), 2);
    }

    #[test]
    fn test_count_lines_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("b.rs");
        std::fs::write(&f, "line1\nline2\n").unwrap();
        assert_eq!(count_lines(&f), 2);
    }
}
