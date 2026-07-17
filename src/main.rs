use anyhow::Result;
use clap::{Parser, Subcommand};

mod cli;
mod config;
mod detectors;
mod git;
mod llm;
mod output;
mod parser;
mod registry;

#[derive(Parser)]
#[command(name = "codasaurus")]
#[command(about = "🦕 AI-generated code verification — catches hallucinated imports, phantom deps, stale APIs, and security issues")]
#[command(version, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check code for AI-generated issues
    Check {
        /// Check staged changes (default)
        #[arg(long)]
        staged: bool,

        /// Check diff against a git ref (e.g. --diff origin/main)
        #[arg(long)]
        diff: Option<String>,

        /// CI mode — JSON output, strict exit codes
        #[arg(long)]
        ci: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Path to config file
        #[arg(long)]
        config: Option<String>,

        /// Path to check (file or directory). Default: staged changes
        path: Option<String>,
    },

    /// Watch mode — live feedback as you code (experimental)
    Watch {
        /// Path to watch
        #[arg(default_value = ".")]
        path: String,
    },

    /// Print version information
    Version,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Check {
            staged,
            diff,
            ci,
            json,
            config: _,
            path,
        } => {
            let cfg = config::load()?;
            let findings = cli::run_check(staged, diff, ci, json, path, &cfg)?;
            output::render(&findings, *json || *ci)?;
            if findings.has_blocking() && (*ci || *staged) {
                std::process::exit(1);
            }
        }
        Commands::Watch { path } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(cli::run_watch(path))?;
        }
        Commands::Version => {
            println!("codasaurus v{}", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}
