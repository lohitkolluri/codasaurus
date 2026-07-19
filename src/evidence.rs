/// Create an ExitStatus representing a killed/timed-out process.
fn killed_exit_status() -> std::process::ExitStatus {
    if cfg!(unix) {
        #[cfg(unix)]
        return std::process::ExitStatus::from_raw(-1);
    }
    // Windows: run "cmd /c exit 1" to get a non-zero exit status.
    std::process::Command::new("cmd")
        .args(["/c", "exit", "1"])
        .status()
        .unwrap_or_else(|_| {
            // On locked-down systems where cmd may be unavailable,
            // use "exit 1" as a string argument to a minimal shell.
            std::process::Command::new("powershell")
                .args(["-Command", "exit 1"])
                .status()
                .unwrap_or(std::process::ExitStatus::from_raw(1))
        })
}

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::time::Duration;

/// A single test execution result — the atomic evidence unit.
#[derive(Debug, Clone, Serialize)]
pub struct TestExecution {
    /// Fully qualified test name (e.g. "my_module::tests::test_validate")
    pub test_name: String,

    /// Exact command that was run
    pub command: String,

    /// Process exit code (0 = passing, non-zero = failing)
    pub exit_code: i32,

    /// Stdout captured from the test run (truncated to 256KB)
    pub stdout: String,

    /// Stderr captured from the test run
    pub stderr: String,

    /// Wall-clock duration in milliseconds
    pub duration_ms: u64,

    /// SHA-256 hash of stdout for tamper-evident evidence
    pub output_hash: String,

    /// Did the test pass?
    pub passed: bool,
}

impl TestExecution {
    pub fn new(
        test_name: String,
        command: String,
        exit_code: i32,
        stdout: String,
        stderr: String,
        duration: Duration,
    ) -> Self {
        let output_hash = {
            let mut hasher = Sha256::new();
            hasher.update(stdout.as_bytes());
            hex::encode(hasher.finalize())
        };
        Self {
            test_name,
            command,
            exit_code,
            duration_ms: duration.as_millis() as u64,
            output_hash,
            passed: exit_code == 0,
            stdout: truncate_output(&stdout, 256 * 1024),
            stderr: truncate_output(&stderr, 64 * 1024),
        }
    }
}

impl fmt::Display for TestExecution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.passed { "PASS" } else { "FAIL" };
        write!(
            f,
            "[{status}] {name} ({dur}ms, exit={code}, hash={hash})",
            status = status,
            name = self.test_name,
            dur = self.duration_ms,
            code = self.exit_code,
            hash = &self.output_hash[..12]
        )
    }
}

/// A symbol that changed in the diff.
#[derive(Debug, Clone, Serialize)]
pub struct ChangedSymbol {
    pub name: String,
    pub file: String,
    pub line: usize,
    pub kind: String, // "function", "struct", "import", "variable", "type"
}

/// A fully structured "fix packet" that can be consumed by humans or AI agents.
///
/// Inspired by CodeRabbit's "Prompt for AI Agents" pattern + Greptile TREX
/// evidence artifacts. Every finding carries proof of execution so a downstream
/// coding agent can act without re-running the entire pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct FixPacket {
    /// File path and line of the finding
    pub file: String,
    pub line: usize,

    /// Severity: "error", "warning", "info"
    pub severity: &'static str,

    /// Short one-line title
    pub title: String,

    /// Human-readable description
    pub description: String,

    /// The symbol that changed and triggered this finding
    pub changed_symbol: String,

    /// Symbols transitively impacted via the call graph
    pub impacted_callers: Vec<String>,

    /// Files containing impacted symbols
    pub impacted_files: Vec<String>,

    /// Test execution evidence (None if tests were skipped)
    pub test_evidence: Option<TestExecution>,

    /// Suggested code patch to fix the issue
    pub suggested_fix: Option<String>,

    /// Copyable prompt for Codex / Claude / Cursor that an AI coding agent
    /// can execute to fix this finding autonomously.
    pub agent_prompt: Option<String>,
}

impl FixPacket {
    /// Build an agent-ready prompt that a downstream coding agent can consume.
    /// Pattern: CodeRabbit's "Prompt for AI Agents" structured blocks.
    pub fn build_agent_prompt(&self) -> String {
        let evidence_block = match &self.test_evidence {
            Some(te) => format!(
                "## Test Evidence\n\
                 - Test: `{}`\n\
                 - Result: {}\n\
                 - Exit code: {}\n\
                 - Duration: {}ms\n\
                 - Command: `{}`\n\
                 - Output hash: `{}`\n",
                te.test_name,
                if te.passed { "PASS" } else { "FAIL" },
                te.exit_code,
                te.duration_ms,
                te.command,
                te.output_hash,
            ),
            None => "## Test Evidence\n- Tests were not executed (use --run-tests to enable).\n".to_string(),
        };

        format!(
            "## Task: Fix Issue in {file}:{line}\n\
             \n\
             ### Issue\n\
             - Severity: {severity}\n\
             - Title: {title}\n\
             - Description: {description}\n\
             \n\
             ### Blast Radius\n\
             - Changed symbol: `{changed_symbol}`\n\
             - Impacted callers: {callers}\n\
             - Impacted files: {files}\n\
             \n\
             {evidence}\
             \n\
             ### Suggested Fix\n\
             {suggested}\
             \n\
             ### Instructions\n\
             Review the issue and apply the suggested fix. \
             Verify the fix by running the impacted tests. \
             Do not modify code outside the blast radius unless necessary.",
            file = self.file,
            line = self.line,
            severity = self.severity,
            title = self.title,
            description = self.description,
            changed_symbol = self.changed_symbol,
            callers = self.impacted_callers.join(", "),
            files = self.impacted_files.join(", "),
            evidence = evidence_block,
            suggested = self.suggested_fix.as_deref().unwrap_or("No automatic fix available."),
        )
    }
}

/// Full report produced by `codasaurus verify`.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    /// Git ref being verified against (e.g. "origin/main")
    pub diff_base: Option<String>,

    /// Files changed in the diff
    pub changed_files: Vec<String>,

    /// Number of changed files
    pub changed_file_count: usize,

    /// Symbols extracted from the changed files
    pub changed_symbols: Vec<ChangedSymbol>,

    /// Symbols transitively impacted
    pub impacted_symbols: Vec<String>,

    /// Files containing impacted symbols
    pub impacted_files: Vec<String>,

    /// Unique impacted files count
    pub impacted_file_count: usize,

    /// Test executions run
    pub test_executions: Vec<TestExecution>,

    /// Test summary
    pub tests_passed: usize,
    pub tests_failed: usize,
    pub tests_skipped: usize,

    /// Fix packets — one per meaningful finding
    pub fix_packets: Vec<FixPacket>,

    /// Did the entire verification pass?
    pub verified: bool,
}

impl VerifyReport {
    pub fn new(diff_base: Option<String>) -> Self {
        Self {
            diff_base,
            changed_files: Vec::new(),
            changed_file_count: 0,
            changed_symbols: Vec::new(),
            impacted_symbols: Vec::new(),
            impacted_files: Vec::new(),
            impacted_file_count: 0,
            test_executions: Vec::new(),
            tests_passed: 0,
            tests_failed: 0,
            tests_skipped: 0,
            fix_packets: Vec::new(),
            verified: true,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.fix_packets.is_empty()
    }

    /// Call after all test executions are collected to update summary.
    pub fn finalize(&mut self) {
        self.tests_passed = self.test_executions.iter().filter(|t| t.passed).count();
        self.tests_failed = self.test_executions.iter().filter(|t| !t.passed).count();
        self.changed_file_count = self.changed_files.len();
        self.impacted_file_count = self.impacted_files.len();
        self.verified = self.tests_failed == 0 && !self.fix_packets.iter().any(|f| f.severity == "error");
    }
}

/// Render a VerifyReport to stdout in human-readable format.
pub fn render_report(report: &VerifyReport, json_mode: bool) -> anyhow::Result<()> {
    if json_mode {
        let output = serde_json::to_string_pretty(&report)?;
        println!("{}", output);
    } else {
        render_terminal(report);
    }
    Ok(())
}

fn render_terminal(report: &VerifyReport) {
    use colored::*;

    println!("  {} {}", "🦕".bold(), "Codasaurus Verify".bold());
    if let Some(ref base) = report.diff_base {
        println!("  Diff base: {}", base.dimmed());
    }
    println!();

    // Change summary
    println!("  {} changes", "Changes:".bold().underline());
    println!("    {} files changed", report.changed_file_count);
    println!("    {} symbols extracted", report.changed_symbols.len());
    println!("    {} symbols in blast radius", report.impacted_symbols.len());
    println!("    {} impacted files", report.impacted_file_count);
    println!();

    // Test summary
    println!("  {}", "Tests:".bold().underline());
    if report.test_executions.is_empty() {
        println!("    {} No tests executed (use --run-tests to enable)", "⚠".yellow());
    } else {
        println!("    {} passed, {} failed, {} skipped",
            report.tests_passed.to_string().green(),
            report.tests_failed.to_string().red(),
            report.tests_skipped,
        );
        for te in &report.test_executions {
            let icon = if te.passed { "✓".green() } else { "✗".red() };
            println!("    {} {} ({}ms)", icon, te.test_name, te.duration_ms);
        }
    }
    println!();

    // Findings
    if report.fix_packets.is_empty() {
        println!("  {}  No issues found — blast radius is clean", "✓".green().bold());
    } else {
        let errors = report.fix_packets.iter().filter(|f| f.severity == "error").count();
        let warnings = report.fix_packets.iter().filter(|f| f.severity == "warning").count();
        let infos = report.fix_packets.iter().filter(|f| f.severity == "info").count();

        let mut summary_parts = vec![];
        if errors > 0 {
            summary_parts.push(format!("{} errors", errors.to_string().red().bold()));
        }
        if warnings > 0 {
            summary_parts.push(format!("{} warnings", warnings.to_string().yellow().bold()));
        }
        if infos > 0 {
            summary_parts.push(format!("{} infos", infos.to_string().cyan().bold()));
        }
        println!("  {} — {}", "Findings".bold().underline(), summary_parts.join(", "));
        println!();

        for fp in &report.fix_packets {
            let sev_char = match fp.severity {
                "error" => "✗".red().bold().to_string(),
                "warning" => "⚠".yellow().bold().to_string(),
                _ => "ℹ".cyan().bold().to_string(),
            };
            let location = if fp.line > 0 {
                format!(":{}", fp.line)
            } else {
                String::new()
            };

            println!("    {} {} [{}]", sev_char, fp.title.dimmed(), format!("{}{}", fp.file, location).dimmed());
            println!("      {}", fp.description);

            if !fp.impacted_callers.is_empty() {
                println!("      {} {}", "→".blue().bold(), format!("Blast radius: {} callers, {} files",
                    fp.impacted_callers.len(),
                    fp.impacted_files.len(),
                ).blue());
            }

            if let Some(ref te) = fp.test_evidence {
                let status = if te.passed { "PASS".green() } else { "FAIL".red() };
                println!("      {} {} — {} ({}ms)", "Test:".bold(), te.test_name, status, te.duration_ms);
            }

            if let Some(ref _prompt) = fp.agent_prompt {
                println!("      {} Available for AI agent fix", "🤖".bold());
            }
            println!();
        }
    }

    // Final verdict
    println!("  {}", "Verdict:".bold().underline());
    let verdict = if report.verified {
        "✓ Verified — blast radius is clean".green().bold()
    } else if report.tests_failed > 0 {
        "✗ Tests failed — changes may break existing behavior".red().bold()
    } else {
        "⚠ Needs review — findings require attention".yellow().bold()
    };
    println!("    {}", verdict);
    println!();
}

fn truncate_output(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        format!("{}... [truncated {} bytes]", &s[..max_bytes], s.len() - max_bytes)
    }
}

/// Execute a shell command with timeout, capture stdout/stderr/exit code.
/// This is the core sandboxed execution primitive for `codasaurus verify`.
pub fn execute_command(
    command: &str,
    args: &[&str],
    timeout: Duration,
    workdir: Option<&str>,
) -> Result<TestExecution, String> {
    let start = std::time::Instant::now();

    let mut cmd = std::process::Command::new(command);
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn `{}`: {}", command, e))?;

    // Wait with timeout
    let (exit_status, stdout, stderr) = {
        let start_wait = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let output = child.wait_with_output().map_err(|e| format!("Failed to collect output: {}", e))?;
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    break (status, stdout, stderr);
                }
                Ok(None) => {
                    if start_wait.elapsed() > timeout {
                        // Kill the child process
                        let _ = child.kill();
                        // Wait for kill to take effect
                        let _ = child.wait();
                        let stdout = String::new();
                        let stderr = format!("Command timed out after {}ms", timeout.as_millis());
                        break (
                            killed_exit_status(),
                            stdout,
                            stderr,
                        );
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    return Err(format!("Failed to wait for command: {}", e));
                }
            }
        }
    };

    let elapsed = start.elapsed();
    let exit_code = exit_status.code().unwrap_or(-1);
    let test_name = args.join(" ");

    Ok(TestExecution::new(
        test_name,
        format!("{} {}", command, args.join(" ")),
        exit_code,
        stdout,
        stderr,
        elapsed,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_execution_display_passed() {
        let te = TestExecution::new(
            "my_test".into(),
            "cargo test my_test".into(),
            0,
            "running 1 test\ntest my_test ... ok".into(),
            "".into(),
            Duration::from_millis(123),
        );
        let s = te.to_string();
        assert!(s.starts_with("[PASS]"));
        assert!(s.contains("my_test"));
        assert!(s.contains("123ms"));
    }

    #[test]
    fn test_test_execution_display_failed() {
        let te = TestExecution::new(
            "my_failing_test".into(),
            "cargo test my_failing_test".into(),
            1,
            "running 1 test\ntest my_failing_test ... FAILED".into(),
            "error: test failed".into(),
            Duration::from_millis(45),
        );
        let s = te.to_string();
        assert!(s.starts_with("[FAIL]"));
    }

    #[test]
    fn test_fix_packet_agent_prompt_includes_evidence() {
        let te = TestExecution::new(
            "mod::test_fn".into(),
            "cargo test mod::test_fn".into(),
            0,
            "ok".into(),
            "".into(),
            Duration::from_millis(100),
        );
        let fp = FixPacket {
            file: "src/main.rs".into(),
            line: 42,
            severity: "warning",
            title: "Unused import".into(),
            description: "`old_thing` is imported but never used after refactor".into(),
            changed_symbol: "old_thing".into(),
            impacted_callers: vec!["caller_a".into()],
            impacted_files: vec!["src/caller_a.rs".into()],
            test_evidence: Some(te),
            suggested_fix: Some("Remove `use old_thing;`".into()),
            agent_prompt: None,
        };
        let prompt = fp.build_agent_prompt();
        assert!(prompt.contains("src/main.rs:42"));
        assert!(prompt.contains("Unused import"));
        assert!(prompt.contains("Test Evidence"));
        assert!(prompt.contains("PASS"));
    }

    #[test]
    fn test_fix_packet_agent_prompt_no_evidence() {
        let fp = FixPacket {
            file: "src/lib.rs".into(),
            line: 10,
            severity: "info",
            title: "Wide blast radius".into(),
            description: "Symbol affects 15 callers".into(),
            changed_symbol: "core_util".into(),
            impacted_callers: (0..15).map(|i| format!("caller_{}", i)).collect(),
            impacted_files: vec!["src/util.rs".into()],
            test_evidence: None,
            suggested_fix: None,
            agent_prompt: None,
        };
        let prompt = fp.build_agent_prompt();
        assert!(prompt.contains("Tests were not executed"));
        assert!(prompt.contains("No automatic fix available"));
    }

    #[test]
    fn test_execute_command_success() {
        let result = execute_command("echo", &["hello"], Duration::from_secs(5), None).unwrap();
        assert!(result.passed);
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn test_execute_command_failure() {
        let result = execute_command("sh", &["-c", "exit 42"], Duration::from_secs(5), None).unwrap();
        assert!(!result.passed);
        assert_eq!(result.exit_code, 42);
    }

    #[test]
    fn test_execute_command_timeout() {
        let result = execute_command("sh", &["-c", "sleep 10"], Duration::from_millis(50), None).unwrap();
        assert!(!result.passed);
        assert!(result.stderr.contains("timed out"));
    }

    #[test]
    fn test_truncate_output() {
        let short = "hello";
        assert_eq!(truncate_output(short, 100), "hello");

        let long = "a".repeat(1000);
        let truncated = truncate_output(&long, 100);
        assert!(truncated.len() < long.len());
        assert!(truncated.contains("truncated"));
    }

    #[test]
    fn test_verify_report_finalize() {
        let mut report = VerifyReport::new(Some("origin/main".into()));
        report.changed_files = vec!["src/main.rs".into()];
        report.test_executions = vec![
            TestExecution::new("passing_test".into(), "".into(), 0, "".into(), "".into(), Duration::from_millis(1)),
            TestExecution::new("failing_test".into(), "".into(), 1, "".into(), "".into(), Duration::from_millis(1)),
        ];
        report.fix_packets = vec![FixPacket {
            file: "src/lib.rs".into(),
            line: 0,
            severity: "error",
            title: "test".into(),
            description: "test".into(),
            changed_symbol: "x".into(),
            impacted_callers: vec![],
            impacted_files: vec![],
            test_evidence: None,
            suggested_fix: None,
            agent_prompt: None,
        }];
        report.finalize();
        assert!(!report.verified);
        assert_eq!(report.tests_passed, 1);
        assert_eq!(report.tests_failed, 1);
    }
}
