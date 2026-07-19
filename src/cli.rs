use crate::config::Config;
use crate::detectors::{self, Findings};
use crate::git;
use crate::parser;
use crate::util;
use anyhow::Result;
use colored::Colorize;
use std::path::Path;

pub struct CheckOptions<'a> {
    pub staged: bool,
    pub diff: Option<String>,
    pub ci: bool,
    pub llm: bool,
    pub json: bool,
    pub path: Option<String>,
    pub config: &'a Config,
    pub quiet: bool,
}

pub fn run_check(opts: CheckOptions) -> Result<Findings> {
    if !git::is_git_repo() && opts.path.is_none() {
        anyhow::bail!(
            "Not in a git repository. Run codasaurus check <path> or use from within a git repo."
        );
    }

    let mut parsed_files = Vec::new();

    if let Some(ref specific_path) = opts.path {
        let p = Path::new(specific_path);
        if p.is_dir() {
            for entry in walkdir::WalkDir::new(p)
                .into_iter()
                .filter_entry(|e| !util::is_hidden(e.path()))
                .filter_map(|e| {
                    if let Err(err) = &e {
                        eprintln!("Warning: error accessing directory entry: {}", err);
                    }
                    e.ok()
                })
            {
                if entry.file_type().is_file() {
                    let path_str = entry.path().to_string_lossy().to_string();
                    collect_parsed_file(&path_str, &mut parsed_files, opts.config);
                }
            }
        } else if p.is_file() {
            let path_str = p.to_string_lossy().to_string();
            collect_parsed_file(&path_str, &mut parsed_files, opts.config);
        }
    } else if opts.staged {
        let diff_output = git::get_staged_diff()?;
        let changed_files = extract_changed_files(&diff_output)?;
        for file_path in &changed_files {
            collect_parsed_file(file_path, &mut parsed_files, opts.config);
        }
    } else if let Some(ref ref_a) = opts.diff {
        let diff_output = git::get_diff_between(ref_a, "HEAD")?;
        let changed_files = extract_changed_files(&diff_output)?;
        for file_path in &changed_files {
            collect_parsed_file(file_path, &mut parsed_files, opts.config);
        }
    }

    // Cross-file detectors need one coherent view of the changed set: running
    // them file-by-file misses manifest relationships and repeats repo checks.
    let mut findings = if parsed_files.is_empty() {
        Findings::new()
    } else {
        detectors::run_all(&parsed_files, opts.config)
    };

    if opts.llm {
        if let Some(llm_cfg) = crate::llm::LlmConfig::from_env() {
            let diff = if opts.staged {
                git::get_staged_diff().unwrap_or_default()
            } else if let Some(ref ref_a) = opts.diff {
                git::get_diff_between(ref_a, "HEAD").unwrap_or_default()
            } else {
                String::new()
            };
            if !diff.is_empty() {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to create async runtime: {}; skipping LLM review",
                            e
                        );
                        return Ok(findings);
                    }
                };

                if !opts.quiet {
                    eprintln!("Running LLM review...");
                }

                let guidelines_override = opts.config.guidelines.contributing_guidelines.as_deref();
                let repo_context_str = git::repo_root()
                    .ok()
                    .and_then(|r| crate::context::build_repo_context(&r, guidelines_override))
                    .map(|c| c.to_string());

                let review_ctx = crate::llm::ReviewContext {
                    repo_context: repo_context_str,
                    ..Default::default()
                };

                let result =
                    rt.block_on(crate::llm::review_diff(&diff, &llm_cfg, Some(&review_ctx)));

                match result {
                    Ok(output) => {
                        for issue in output.issues {
                            findings.findings.push(crate::detectors::Finding {
                                detector: "llm-review".to_string(),
                                severity: match issue.severity.as_str() {
                                    "critical" | "high" => "blocking",
                                    "warning" | "moderate" | "medium" => "warning",
                                    _ => "info",
                                },
                                file: issue.file,
                                line: issue.line,
                                column: 0,
                                message: issue.description,
                                suggestion: issue.suggestion,
                                evidence: None,
                                codemod: None,
                            });
                        }
                    }
                    Err(e) => {
                        eprintln!("LLM review failed: {}", e);
                    }
                }
            }
        } else {
            eprintln!("--llm flag set but no API key found. Set OPENROUTER_API_KEY or CODASAURUS_API_KEY.");
        }
    }

    Ok(findings)
}

pub async fn run_watch(path: &str, config_path: Option<&str>) -> Result<()> {
    use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
    use std::sync::mpsc;
    use std::time::Duration;

    println!("🦕 Codasaurus watching {}... (Ctrl+C to stop)", path);

    let (tx, rx) = mpsc::channel::<Result<Event, notify::Error>>();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(Path::new(path), RecursiveMode::Recursive)?;

    // Debounce: wait for 500ms of no changes before checking
    let mut last_check = std::time::Instant::now();

    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(_event)) => {
                last_check = std::time::Instant::now();
            }
            Ok(Err(e)) => {
                eprintln!("Watch error: {}", e);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Check if enough time has passed since last change
                if last_check.elapsed() >= Duration::from_millis(500) {
                    let diff = git::get_staged_diff().unwrap_or_default();
                    if !diff.is_empty() {
                        let config = match crate::config::load(config_path) {
                            Ok(c) => c,
                            Err(e) => {
                                eprintln!(
                                    "Warning: Could not load config file: {}; using defaults",
                                    e
                                );
                                crate::config::Config::default()
                            }
                        };
                        let findings = run_check(CheckOptions {
                            staged: true,
                            diff: None,
                            ci: false,
                            llm: false,
                            json: false,
                            path: None,
                            config: &config,
                            quiet: true,
                        })
                        .unwrap_or_default();

                        print!("\x1B[2J\x1B[H"); // Clear screen
                        if findings.is_empty() {
                            println!("{} No issues", "✓".green());
                        } else {
                            crate::output::render(&findings, false)?;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}

fn collect_parsed_file(
    file_path: &str,
    parsed_files: &mut Vec<parser::ParsedFile>,
    config: &Config,
) {
    if !parser::is_supported(file_path) {
        return;
    }
    if detectors::is_excluded(file_path, &config.checks.exclude_patterns) {
        return;
    }
    match std::fs::read_to_string(file_path) {
        Ok(content) => match parser::parse_file(file_path, &content) {
            Ok(parsed) => parsed_files.push(parsed),
            Err(e) => eprintln!("Warning: failed to parse file {}: {}", file_path, e),
        },
        Err(e) => eprintln!("Warning: failed to read file {}: {}", file_path, e),
    }
}

fn extract_changed_files(diff_output: &str) -> Result<Vec<String>> {
    let mut files = Vec::new();

    for line in diff_output.lines() {
        // Match lines like: "+++ b/path/to/file"
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
