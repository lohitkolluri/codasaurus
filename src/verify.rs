use crate::config::Config;
use crate::detectors::graph as code_graph;
use crate::evidence::{execute_command, ChangedSymbol, FixPacket, TestExecution, VerifyReport};
use crate::git;
use crate::parser;
use crate::util;
use anyhow::Result;
use std::time::Duration;

/// CLI options for `codasaurus verify`.
pub struct VerifyOptions<'a> {
    pub staged: bool,
    pub diff: Option<String>,
    pub path: Option<String>,
    pub run_tests: bool,
    pub force: bool,
    pub ci: bool,
    pub json: bool,
    pub config: &'a Config,
}

/// Main entry point for `codasaurus verify`.
pub fn run_verify(opts: VerifyOptions) -> Result<VerifyReport> {
    let diff_base = opts.diff.clone().or_else(|| {
        if opts.staged {
            Some("staged".to_string())
        } else {
            None
        }
    });

    let mut report = VerifyReport::new(diff_base);

    // Step 1: Get the diff / changed files
    let changed_files = get_changed_files(&opts)?;
    report.changed_files = changed_files.clone();

    if changed_files.is_empty() {
        return Ok(report);
    }

    // Step 2: Parse changed files to extract symbols
    let parsed_files = parse_changed_files(&changed_files, opts.config);
    if parsed_files.is_empty() {
        return Ok(report);
    }

    // Step 3: Build the code graph from the changed files
    let graph = build_verify_graph(&parsed_files)?;

    // Step 4: Extract changed symbols from the graph
    let changed_symbols = extract_changed_symbols(&graph, &changed_files);
    report.changed_symbols = changed_symbols;

    if report.changed_symbols.is_empty() {
        return Ok(report);
    }

    // Step 5: Trace blast radius for each changed symbol
    let changed_sym_names: Vec<String> = report
        .changed_symbols
        .iter()
        .map(|s| s.name.clone())
        .collect();

    let impacted_syms = trace_blast_radius(&graph, &changed_sym_names);
    report.impacted_symbols = impacted_syms.clone();
    report.impacted_files = graph.affected_files(&changed_sym_names);

    // Step 6: Map impacted symbols to test names
    let test_names = map_symbols_to_tests(&graph, &impacted_syms, &changed_files);

    // Step 7: Run tests (if requested)
    if opts.run_tests || opts.force {
        if !test_names.is_empty() {
            let executions = run_targeted_tests(&test_names, &opts)?;
            report.test_executions = executions;
        }

        // Also run the full test for the changed modules
        let module_tests = extract_module_tests(&changed_files);
        for module_test in module_tests {
            // Check if we already have an execution for this test
            let already_run = report
                .test_executions
                .iter()
                .any(|t| t.test_name == module_test);
            if !already_run {
                let result = run_single_test(&module_test);
                match result {
                    Ok(te) => report.test_executions.push(te),
                    Err(e) => {
                        eprintln!(
                            "Warning: test execution failed for '{}': {}",
                            module_test, e
                        );
                    }
                }
            }
        }
    }

    // Step 8: Build fix packets from the analysis
    report.fix_packets = build_fix_packets(
        &graph,
        &report.changed_symbols,
        &impacted_syms,
        &report.impacted_files,
        &report.test_executions,
    );

    report.finalize();
    Ok(report)
}

/// Get the list of changed files from the diff or provided path.
fn get_changed_files(opts: &VerifyOptions) -> Result<Vec<String>> {
    if let Some(ref specific_path) = opts.path {
        let p = std::path::Path::new(specific_path);
        if p.is_file() {
            return Ok(vec![specific_path.to_string()]);
        } else if p.is_dir() {
            // Walk directory for supported files
            let mut files = Vec::new();
            for entry in walkdir::WalkDir::new(p)
                .into_iter()
                .filter_entry(|e| !util::is_hidden(e.path()))
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file() {
                    let path_str = entry.path().to_string_lossy().to_string();
                    if parser::is_supported(&path_str) {
                        files.push(path_str);
                    }
                }
            }
            return Ok(files);
        }
        return Ok(vec![]);
    }

    if opts.staged {
        let diff_output = git::get_staged_diff()?;
        return extract_changed_files_from_diff(&diff_output);
    }

    if let Some(ref ref_a) = opts.diff {
        let diff_output = git::get_diff_between(ref_a, "HEAD")?;
        return extract_changed_files_from_diff(&diff_output);
    }

    Ok(vec![])
}

/// Parse the list of changed files to extract symbols.
fn parse_changed_files(files: &[String], config: &Config) -> Vec<parser::ParsedFile> {
    let mut parsed = Vec::new();
    for file_path in files {
        if !parser::is_supported(file_path) {
            continue;
        }
        if crate::detectors::is_excluded(file_path, &config.checks.exclude_patterns) {
            continue;
        }
        match std::fs::read_to_string(file_path) {
            Ok(content) => match parser::parse_file(file_path, &content) {
                Ok(pf) => parsed.push(pf),
                Err(e) => eprintln!("Warning: failed to parse file {}: {}", file_path, e),
            },
            Err(e) => eprintln!("Warning: failed to read file {}: {}", file_path, e),
        }
    }
    parsed
}

/// Build a code graph from parsed files for verification purposes.
/// Reuses the same global graph that the detectors module populates.
fn build_verify_graph(
    parsed_files: &[parser::ParsedFile],
) -> anyhow::Result<std::sync::MutexGuard<'static, crate::graph::CodeGraph>> {
    code_graph::populate(parsed_files);
    code_graph::lock_graph().map_err(|e| anyhow::anyhow!("Code graph unavailable: {}", e))
}

/// Extract changed symbols from the graph for the given files.
fn extract_changed_symbols<'a>(
    graph: &std::sync::MutexGuard<'a, crate::graph::CodeGraph>,
    changed_files: &[String],
) -> Vec<ChangedSymbol> {
    let mut symbols = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for file in changed_files {
        let file_syms = graph.symbols_in_file(file);
        for sym_name in file_syms {
            if seen.insert(sym_name.to_string()) {
                let kind = if sym_name.contains("::") {
                    // Symbol has a path prefix
                    "import"
                } else {
                    "variable"
                };

                symbols.push(ChangedSymbol {
                    name: sym_name.to_string(),
                    file: file.clone(),
                    line: 0, // Line-level tracking todo: extract from graph
                    kind: kind.to_string(),
                });
            }
        }
    }

    symbols
}

/// Trace blast radius for a set of symbols.
fn trace_blast_radius(
    graph: &std::sync::MutexGuard<'_, crate::graph::CodeGraph>,
    symbols: &[String],
) -> Vec<String> {
    let mut impacted = std::collections::BTreeSet::new();
    for sym in symbols {
        let radius = graph.blast_radius(sym, 3);
        for node in radius {
            impacted.insert(node.name.clone());
        }
    }
    impacted.into_iter().collect()
}

/// Map symbols to potential test names using the graph's heuristic.
fn map_symbols_to_tests(
    graph: &std::sync::MutexGuard<'_, crate::graph::CodeGraph>,
    impacted_symbols: &[String],
    _changed_files: &[String],
) -> Vec<String> {
    let mut test_names = std::collections::BTreeSet::new();
    for sym in impacted_symbols {
        let names = graph.symbol_to_test_names(sym);
        for name in names {
            test_names.insert(name);
        }
    }
    test_names.into_iter().collect()
}

/// Extract module-level test names from changed files.
/// e.g. "src/my_module.rs" -> "my_module"
fn extract_module_tests(changed_files: &[String]) -> Vec<String> {
    let mut tests = std::collections::BTreeSet::new();
    for file in changed_files {
        // Extract module name from file path
        let path = std::path::Path::new(file);
        let module = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if !module.is_empty() {
            // Rust convention: module-level test
            tests.insert(format!("{}::", module));
        }
    }
    tests.into_iter().collect()
}

/// Run targeted tests and collect evidence.
fn run_targeted_tests(
    test_names: &[String],
    opts: &VerifyOptions,
) -> Result<Vec<TestExecution>, anyhow::Error> {
    // Prompt user for approval unless --force
    if !opts.force && !opts.ci {
        let test_list = test_names.join(", ");
        eprintln!(
            "Codasaurus will run the following tests:\n  {}\nRun tests? [y/N] ",
            test_list
        );
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            eprintln!("Test execution skipped.");
            return Ok(Vec::new());
        }
    }

    let mut executions = Vec::new();
    for test_name in test_names {
        let result = run_single_test(test_name);
        match result {
            Ok(te) => executions.push(te),
            Err(e) => {
                eprintln!("Warning: could not run test '{}': {}", test_name, e);
            }
        }
    }

    Ok(executions)
}

/// Run a single test and capture evidence.
fn run_single_test(test_name: &str) -> Result<TestExecution, String> {
    // Detect test framework from project structure
    let (command, args) = if is_rust_project() {
        if test_name.ends_with("::") {
            // Module-level: cargo test module::
            ("cargo", vec!["test", "--quiet", test_name])
        } else {
            ("cargo", vec!["test", "--quiet", "--", "--exact", test_name])
        }
    } else if is_node_project() {
        if test_name.ends_with(".test") || test_name.ends_with(".spec") {
            ("npx", vec!["jest", test_name])
        } else {
            ("npx", vec!["jest", "--testNamePattern", test_name])
        }
    } else if is_python_project() {
        ("python", vec!["-m", "pytest", "-k", test_name])
    } else {
        ("cargo", vec!["test", "--quiet", test_name])
    };

    execute_command(
        command,
        &args,
        Duration::from_secs(120), // 2 minute timeout per test
        None,                     // Use repo root
    )
}

/// Build fix packets from the analysis results.
fn build_fix_packets(
    graph: &std::sync::MutexGuard<'_, crate::graph::CodeGraph>,
    changed_symbols: &[ChangedSymbol],
    impacted_symbols: &[String],
    impacted_files: &[String],
    test_executions: &[TestExecution],
) -> Vec<FixPacket> {
    let mut packets = Vec::new();

    for changed in changed_symbols {
        // Find callers of this symbol
        let callers: Vec<String> = graph
            .find_callers(&changed.name)
            .iter()
            .map(|n| n.name.clone())
            .collect();

        // Find test evidence for this symbol
        let test_evidence = find_test_evidence(changed, test_executions);

        // Calculate blast radius size
        let radius = graph.blast_radius(&changed.name, 3);
        let radius_size = radius.len();

        // Build the agent prompt
        let mut fp = FixPacket {
            file: changed.file.clone(),
            line: changed.line,
            severity: if radius_size > 10 {
                "warning"
            } else if radius_size > 5 {
                "info"
            } else {
                "info"
            },
            title: format!("Symbol `{}` has blast radius of {} symbols", changed.name, radius_size),
            description: format!(
                "Changed symbol `{}` in `{}` has {} callers and affects {} symbols across {} files.",
                changed.name,
                changed.file,
                callers.len(),
                radius_size,
                impacted_files.len(),
            ),
            changed_symbol: changed.name.clone(),
            impacted_callers: callers,
            impacted_files: impacted_files.to_vec(),
            test_evidence: test_evidence.cloned(),
            suggested_fix: None,
            agent_prompt: None,
        };

        let prompt = fp.build_agent_prompt();
        fp.agent_prompt = Some(prompt);

        packets.push(fp);
    }

    // If no changed symbols but we have impacted symbols, build from those
    if packets.is_empty() && !impacted_symbols.is_empty() {
        for sym in impacted_symbols.iter().take(5) {
            // Limit to 5
            let callers: Vec<String> = graph
                .find_callers(sym)
                .iter()
                .map(|n| n.name.clone())
                .collect();

            let mut fp = FixPacket {
                file: String::new(),
                line: 0,
                severity: "info",
                title: format!("Symbol `{}` is in the blast radius", sym),
                description: format!(
                    "Symbol `{}` is transitively affected by the changed files, with {} direct callers.",
                    sym,
                    callers.len(),
                ),
                changed_symbol: sym.clone(),
                impacted_callers: callers,
                impacted_files: impacted_files.to_vec(),
                test_evidence: None,
                suggested_fix: None,
                agent_prompt: None,
            };
            let prompt = fp.build_agent_prompt();
            fp.agent_prompt = Some(prompt);
            packets.push(fp);
        }
    }

    packets
}

/// Find test evidence matching a changed symbol.
fn find_test_evidence<'a>(
    changed: &ChangedSymbol,
    test_executions: &'a [TestExecution],
) -> Option<&'a TestExecution> {
    let base_name = changed.name.rsplit("::").next().unwrap_or(&changed.name);
    for te in test_executions {
        if te.test_name.contains(base_name) {
            return Some(te);
        }
    }
    None
}

/// Extract changed files from a unified diff.
fn extract_changed_files_from_diff(diff_output: &str) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for line in diff_output.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            let path = path.trim();
            if !path.is_empty() && path != "/dev/null" {
                files.push(path.to_string());
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Check if the current directory is a Rust project.
fn is_rust_project() -> bool {
    std::path::Path::new("Cargo.toml").exists()
}

/// Check if the current directory is a Node project.
fn is_node_project() -> bool {
    std::path::Path::new("package.json").exists()
}

/// Check if the current directory is a Python project.
fn is_python_project() -> bool {
    std::path::Path::new("setup.py").exists()
        || std::path::Path::new("pyproject.toml").exists()
        || std::path::Path::new("requirements.txt").exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_extract_changed_files_from_diff() {
        let diff = "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-foo\n+bar\n";
        let files = extract_changed_files_from_diff(diff).unwrap();
        assert_eq!(files, vec!["src/main.rs"]);
    }

    #[test]
    fn test_extract_changed_files_skips_dev_null() {
        let diff = "--- /dev/null\n+++ /dev/null\n";
        let files = extract_changed_files_from_diff(diff).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_extract_changed_files_multiple() {
        let diff = "--- a/a.rs\n+++ b/a.rs\n--- a/b.rs\n+++ b/b.rs\n";
        let files = extract_changed_files_from_diff(diff).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.contains(&"a.rs".to_string()));
        assert!(files.contains(&"b.rs".to_string()));
    }

    #[test]
    fn test_is_rust_project() {
        // This will only be true when running tests from the project root
        let result = is_rust_project();
        // We can't assert on the result since it depends on CWD, but it shouldn't panic
        let _ = result;
    }

    #[test]
    fn test_map_symbols_to_tests_no_symbols() {
        let graph = crate::graph::CodeGraph::new();
        let graph_locked = std::sync::Mutex::new(graph);
        let guard = graph_locked.lock().unwrap();
        let tests = map_symbols_to_tests(&guard, &[], &[]);
        assert!(tests.is_empty());
    }

    #[test]
    fn test_extract_module_tests() {
        let tests = extract_module_tests(&[
            "src/main.rs".into(),
            "src/lib.rs".into(),
            "src/cli.rs".into(),
        ]);
        assert!(tests.contains(&"main::".to_string()));
        assert!(tests.contains(&"lib::".to_string()));
        assert!(tests.contains(&"cli::".to_string()));
    }

    #[test]
    fn test_build_fix_packets_empty() {
        let graph = crate::graph::CodeGraph::new();
        let graph_locked = std::sync::Mutex::new(graph);
        let guard = graph_locked.lock().unwrap();
        let packets = build_fix_packets(&guard, &[], &[], &[], &[]);
        assert!(packets.is_empty());
    }

    #[test]
    fn test_get_changed_files_with_path_file() {
        // Test that providing a path to an existing file works
        let config = Config::default();
        let opts = VerifyOptions {
            staged: false,
            diff: None,
            path: Some("Cargo.toml".into()),
            run_tests: false,
            force: false,
            ci: false,
            json: false,
            config: &config,
        };
        let result = get_changed_files(&opts);
        assert!(result.is_ok());
        let files = result.unwrap();
        assert!(files.contains(&"Cargo.toml".to_string()));
    }
}
