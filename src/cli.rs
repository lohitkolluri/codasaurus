use crate::config::Config;
use crate::detectors::{self, Findings};
use crate::git;
use crate::parser;
use anyhow::Result;
use colored::Colorize;
use std::path::Path;

pub fn run_check(
    staged: &bool,
    diff: &Option<String>,
    _ci: &bool,
    _llm: &bool,
    _json: &bool,
    path: &Option<String>,
    _config: &Config,
) -> Result<Findings> {
    if !git::is_git_repo() && !path.is_some() {
        anyhow::bail!("Not in a git repository. Run codasaurus check <path> or use from within a git repo.");
    }

    let mut findings = Findings::new();

    if let Some(specific_path) = path {
        // Check a specific file or directory
        let p = Path::new(specific_path);
        if p.is_dir() {
            for entry in walkdir::WalkDir::new(p)
                .into_iter()
                .filter_entry(|e| !is_hidden(e))
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file() {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        let path_str = entry.path().to_string_lossy().to_string();
                        match parser::parse_file(&path_str, &content) {
                            Ok(parsed) => {
                                findings.findings.extend(detectors::run_all(&[parsed], &Config::default()).findings);
                            }
                            Err(_) => continue,
                        }
                    }
                }
            }
        } else if p.is_file() {
            let content = std::fs::read_to_string(p)?;
            let path_str = p.to_string_lossy().to_string();
            let parsed = parser::parse_file(&path_str, &content)?;
            findings.findings.extend(detectors::run_all(&[parsed], &Config::default()).findings);
        }
    } else if *staged || (diff.is_none() && !staged) {
        // Check staged changes (default)
        let diff_output = git::get_staged_diff()?;
        let changed_files = extract_changed_files(&diff_output)?;

        for file_path in &changed_files {
            if !parser::is_supported(file_path) {
                continue;
            }
            match git::get_file_content(file_path) {
                Ok(content) => {
                    match parser::parse_file(file_path, &content) {
                        Ok(parsed) => {
                            findings
                                .findings.extend(detectors::run_all(&[parsed], &Config::default()).findings);
                        }
                        Err(_) => continue,
                    }
                }
                Err(_) => continue,
            }
        }
    } else if let Some(ref_a) = diff {
        // Check diff between refs (for CI)
        let diff_output = git::get_diff_between(ref_a, "HEAD")?;
        let changed_files = extract_changed_files(&diff_output)?;

        for file_path in &changed_files {
            if !parser::is_supported(file_path) {
                continue;
            }
            match git::get_file_content(file_path) {
                Ok(content) => {
                    match parser::parse_file(file_path, &content) {
                        Ok(parsed) => {
                            findings
                                .findings.extend(detectors::run_all(&[parsed], &Config::default()).findings);
                        }
                        Err(_) => continue,
                    }
                }
                Err(_) => continue,
            }
        }
    }

    // Run LLM review if flag is set
    if *_llm {
        if let Some(llm_cfg) = crate::llm::LlmConfig::from_env() {
            let diff = if *staged {
                git::get_staged_diff().unwrap_or_default()
            } else if let Some(ref_a) = diff {
                git::get_diff_between(ref_a, "HEAD").unwrap_or_default()
            } else {
                String::new()
            };
            if !diff.is_empty() {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(r) => r,
                    Err(_) => return Ok(findings),
                };
                match rt.block_on(crate::llm::review_diff(&diff, &llm_cfg, None)) {
                    Ok(output) => {
                        for issue in output.issues {
                            findings.findings.push(crate::detectors::Finding {
                                detector: "llm-review".to_string(),
                                severity: issue.severity,
                                file: issue.file,
                                line: issue.line,
                                column: 0,
                                message: issue.description,
                                suggestion: issue.suggestion,
                                evidence: None,
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

/// Run watch mode — monitor files for changes
pub async fn run_watch(path: &str) -> Result<()> {
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
                        let findings = run_check(
                            &true,
                            &None,
                            &false,
                            &false,
                            &false,
                            &None,
                            &crate::config::Config::default(),
                        )
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

/// Extract list of changed files from a git diff output
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

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.') && s != ".")
        .unwrap_or(false)
}
