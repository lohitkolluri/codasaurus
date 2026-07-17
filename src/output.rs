use crate::detectors::{Finding, Findings};
use colored::*;
use colored::Color;

/// Render findings to terminal or JSON
pub fn render(findings: &Findings, json_mode: bool) -> anyhow::Result<()> {
    if json_mode {
        render_json(findings)?;
    } else {
        render_terminal(findings)?;
    }
    Ok(())
}

fn render_json(findings: &Findings) -> anyhow::Result<()> {
    let output = serde_json::to_string_pretty(&findings)?;
    println!("{}", output);
    Ok(())
}

fn render_terminal(findings: &Findings) -> anyhow::Result<()> {
    if findings.is_empty() {
        println!(
            "{}  No issues found. Code looks clean!",
            "✓".green().bold()
        );
        return Ok(());
    }

    // Group findings by file
    let mut by_file: std::collections::BTreeMap<&str, Vec<&Finding>> =
        std::collections::BTreeMap::new();
    for f in &findings.findings {
        by_file.entry(f.file.as_str()).or_default().push(f);
    }

    // Summary header
    let counts = findings.count_by_severity();
    let blocking = counts.get("blocking").unwrap_or(&0);
    let warnings = counts.get("warning").unwrap_or(&0);
    let infos = counts.get("info").unwrap_or(&0);

    println!(
        "{} Codasaurus found {} issue(s):",
        "🦕".bold(),
        findings.findings.len()
    );
    if *blocking > 0 {
        println!("  {} blocking", blocking.to_string().red().bold());
    }
    if *warnings > 0 {
        println!("  {} warnings", warnings.to_string().yellow().bold());
    }
    if *infos > 0 {
        println!("  {} info", infos.to_string().cyan().bold());
    }
    println!();

    // Findings per file
    for (file, file_findings) in &by_file {
        println!("{}", format!("📁 {}", file).bold().underline());
        println!();

        for finding in file_findings {
            let (symbol, severity_color) = match finding.severity.as_str() {
                "blocking" => ("✗".to_string(), Color::Red),
                "warning" => ("⚠".to_string(), Color::Yellow),
                _ => ("ℹ".to_string(), Color::Cyan),
            };

            let location = if finding.line > 0 {
                format!(":{}", finding.line)
            } else {
                String::new()
            };

            println!(
                "  {} [{}]{}",
                symbol.color(severity_color).bold(),
                finding.detector.bold(),
                location.dimmed()
            );
            println!("    {}", finding.message);

            if let Some(evidence) = &finding.evidence {
                println!("    {}", evidence.dimmed());
            }

            if let Some(suggestion) = &finding.suggestion {
                println!(
                    "    {} {}",
                    "→".blue().bold(),
                    suggestion.blue()
                );
            }

            println!();
        }
    }

    // Summary footer
    if *blocking > 0 {
        println!(
            "{} Found {} blocking issue(s). Fix them before committing.",
            "✗".red().bold(),
            blocking
        );
    } else if *warnings > 0 {
        println!(
            "{} Found {} warning(s). Review recommended.",
            "⚠".yellow().bold(),
            warnings
        );
    }

    Ok(())
}
