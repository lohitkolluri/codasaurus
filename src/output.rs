use crate::detectors::{Finding, Findings};
use colored::*;

/// Render findings to terminal or JSON
pub fn render(findings: &Findings, json_mode: bool) -> anyhow::Result<()> {
    if json_mode {
        let output = serde_json::to_string_pretty(&findings)?;
        println!("{}", output);
    } else {
        render_terminal(findings);
    }
    Ok(())
}

fn render_terminal(findings: &Findings) {
    if findings.is_empty() {
        println!("  ✓  No issues found");
        return;
    }

    let counts = findings.count_by_severity();
    let blocking = counts.get("blocking").copied().unwrap_or(0);
    let warnings = counts.get("warning").copied().unwrap_or(0);
    let infos = counts.get("info").copied().unwrap_or(0);

    // One-line summary
    let mut summary_parts = vec![];
    if blocking > 0 { summary_parts.push(format!("{} blocking", blocking.to_string().red().bold())); }
    if warnings > 0 { summary_parts.push(format!("{} warnings", warnings.to_string().yellow().bold())); }
    if infos > 0 { summary_parts.push(format!("{} infos", infos.to_string().cyan().bold())); }
    println!("  {} {} — {}", "🦕".bold(), "Codasaurus".bold(), summary_parts.join(", "));
    println!();

    // Sort findings by file then line
    let mut sorted = findings.findings.clone();
    sorted.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));

    let mut current_file = String::new();

    for f in &sorted {
        // Print file header when file changes
        if f.file != current_file {
            if !current_file.is_empty() {
                println!();
            }
            current_file = f.file.clone();
            println!("  {}", current_file.bold());
        }

        let sev_char = match f.severity.as_str() {
            "blocking" => "✗".red().bold().to_string(),
            "warning" => "⚠".yellow().bold().to_string(),
            _ => "ℹ".cyan().bold().to_string(),
        };

        let location = if f.line > 0 {
            format!(":{}", f.line)
        } else {
            String::new()
        };

        // Line 1: severity + detector + location
        println!(
            "    {} {} [{}]",
            sev_char,
            f.detector.dimmed(),
            location.dimmed(),
        );

        // Line 2: message
        println!("      {}", f.message);

        // Line 3: fix (if exists)
        if let Some(suggestion) = &f.suggestion {
            println!("      {} {}", "→".blue().bold(), suggestion.blue());
        }
    }
    println!();
}
