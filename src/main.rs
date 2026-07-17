use anyhow::Result;
use clap::{Parser, Subcommand};
use codasaurus::bot;
use codasaurus::cli;
use codasaurus::config;
use codasaurus::output;

#[derive(Parser)]
#[command(name = "codasaurus")]
#[command(
    about = "🦕 AI-generated code verification — catches hallucinated imports, phantom deps, stale APIs, and security issues"
)]
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

        /// Enable LLM-powered deep review (requires API key)
        #[arg(long)]
        llm: bool,

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

    /// Start the GitHub App bot server
    Serve {
        /// Port to listen on
        #[arg(long, default_value = "3000")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "0.0.0.0")]
        host: String,
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
            llm,
            json,
            config: _,
            path,
        } => {
            let cfg = config::load()?;
            let findings = cli::run_check(staged, diff, ci, llm, json, path, &cfg)?;
            output::render(&findings, *json || *ci)?;
            if findings.has_blocking() && (*ci || *staged) {
                std::process::exit(1);
            }
        }
        Commands::Watch { path } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(cli::run_watch(path))?;
        }
        Commands::Serve { port, host } => {
            let config = bot::BotConfig {
                app_id: std::env::var("GITHUB_APP_ID")
                    .map_err(|_| anyhow::anyhow!("GITHUB_APP_ID required"))?,
                private_key: std::env::var("GITHUB_APP_PRIVATE_KEY")
                    .map_err(|_| anyhow::anyhow!("GITHUB_APP_PRIVATE_KEY required"))?,
                webhook_secret: std::env::var("GITHUB_WEBHOOK_SECRET")
                    .map_err(|_| anyhow::anyhow!("GITHUB_WEBHOOK_SECRET required"))?,
                host: host.clone(),
                port: *port,
            };
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(bot::serve(config))?;
        }
        Commands::Version => {
            println!("codasaurus v{}", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}
