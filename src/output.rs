use crate::detectors::{Finding, Findings};
use colored::*;
use colored::Color;
use once_cell::sync::Lazy;
use syntect::easy::HighlightLines;
use syntect::highlighting::{ThemeSet, Style};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(|| {
    SyntaxSet::load_defaults_newlines()
});

static THEME_SET: Lazy<ThemeSet> = Lazy::new(|| {
    ThemeSet::load_defaults()
});

/// Render findings to terminal or JSON
pub fn render(findings: &Findings, json_mode: bool) -> anyhow::Result<()> {
    if json_mode {
        render_json(findings)?;
    } else {
        render_terminal(findings)?;
    }
    Ok(())
}

/// Syntax-highlight a snippet of code for terminal output.
/// Falls back to plain text if highlighting fails.
fn highlight_code(code: &str, file_path: &str) -> String {
    let extension = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let syntax = SYNTAX_SET
        .find_syntax_by_extension(extension)
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

    let theme = &THEME_SET.themes["base16-ocean.dark"];

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut result = String::new();

    for line in LinesWithEndings::from(code) {
        if let Ok(ranges) = highlighter.highlight_line(line, &SYNTAX_SET) {
            let escaped = syntect::util::as_24_bit_terminal_escaped(&ranges[..], true);
            result.push_str(&escaped);
        } else {
            result.push_str(line);
        }
    }

    if result.is_empty() { code.to_string() } else { result }
}

fn render_json(findings: &Findings) -> anyhow::Result<()> {
    let output = serde_json::to_string_pretty(&findings)?;
    println!("{}", output);
    Ok(())
}

fn render_terminal(findings: &Findings) -> anyhow::Result<()> {
    if findings.is_empty() {
        println!();
        println!("  {}  No issues found. Code looks clean!", "✓".green().bold());
        println!();
        return Ok(());
    }

    let mut by_file: std::collections::BTreeMap<&str, Vec<&Finding>> =
        std::collections::BTreeMap::new();
    for f in &findings.findings {
        by_file.entry(f.file.as_str()).or_default().push(f);
    }

    let counts = findings.count_by_severity();
    let blocking = counts.get("blocking").unwrap_or(&0);
    let warnings = counts.get("warning").unwrap_or(&0);
    let infos = counts.get("info").unwrap_or(&0);

    // ── Summary header ──
    println!();
    println!(
        "  {}  Codasaurus — {} issue(s)\n",
        "🦕".bold(),
        findings.findings.len()
    );
    if *blocking > 0 {
        println!(
            "    {}  {} blocking  {}{}",
            "●".red().bold(),
            blocking,
            "must fix before commit".dimmed(),
            if *blocking == 1 { "" } else { "" }
        );
    }
    if *warnings > 0 {
        println!(
            "    {}  {} warning{}  {}",
            "●".yellow().bold(),
            warnings,
            if *warnings == 1 { "" } else { "s" },
            "review recommended".dimmed()
        );
    }
    if *infos > 0 {
        println!(
            "    {}  {} info{}",
            "●".cyan().bold(),
            infos,
            if *infos == 1 { "" } else { "s" },
        );
    }
    println!();

    // ── Findings per file ──
    for (file, file_findings) in &by_file {
        let total = file_findings.len();
        let blocker_count = file_findings.iter().filter(|f| f.severity == "blocking").count();
        let warn_count = file_findings.iter().filter(|f| f.severity == "warning").count();

        let file_line = if blocker_count > 0 {
            format!(
                "{} {}  {} ({} blocking, {} warning{})",
                "📁".bold(),
                file.bold(),
                "─".repeat(4).dimmed(),
                blocker_count.to_string().red().bold(),
                warn_count,
                if warn_count == 1 { "" } else { "s" },
            )
        } else {
            format!(
                "{} {}  {} ({} warning{})",
                "📁".bold(),
                file.bold(),
                "─".repeat(4).dimmed(),
                warn_count,
                if warn_count == 1 { "" } else { "s" },
            )
        };
        println!("  {}", file_line);
        println!();

        for finding in file_findings {
            let severity_tag = match finding.severity.as_str() {
                "blocking" => format!("{} BLOCKING", "●".red()),
                "warning" => format!("{} WARNING", "●".yellow()),
                _ => format!("{} INFO", "●".cyan()),
            };

            let location = if finding.line > 0 {
                format!(" {}:{} ", "at".dimmed(), finding.line.to_string().bold())
            } else {
                String::new()
            };

            // Detector badge
            let badge = format!("[{}]", finding.detector.bold());

            // Severity + detector + location header
            println!(
                "    {}  {} {}",
                severity_tag,
                badge,
                location,
            );

            // Message — wrap at terminal width
            println!("       {}", finding.message);

            // Evidence / code context — syntax-highlighted
            if let Some(evidence) = &finding.evidence {
                println!();
                let highlighted = highlight_code(evidence, &finding.file);
                println!("       {} {}", "code:".dimmed(), highlighted);
            }

            // Suggestion
            if let Some(suggestion) = &finding.suggestion {
                println!(
                    "       {} {}",
                    "fix:".blue().bold(),
                    suggestion.blue()
                );
            }
            println!();
        }
    }

    // ── Summary footer ──
    let sep = "─".repeat(48);
    println!("  {}", sep.dimmed());
    if *blocking > 0 {
        println!(
            "  {} {} blocking — fix before commit",
            "✗".red().bold(),
            blocking
        );
    }
    if *warnings > 0 {
        println!(
            "  {} {} warning{} — review recommended",
            "⚠".yellow().bold(),
            warnings,
            if *warnings == 1 { "" } else { "s" },
        );
    }
    if *blocking == 0 && *warnings == 0 {
        println!("  {} All clear — no action needed", "✓".green().bold());
    }
    println!();

    Ok(())
}
