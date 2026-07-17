use anyhow::Result;
use clap::{Parser, Subcommand};
use codasaurus::bot;
use codasaurus::cli;
use codasaurus::config;
use codasaurus::interactive;
use codasaurus::output;
use std::io::IsTerminal;

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

        /// Path to config file
        #[arg(long)]
        config: Option<String>,
    },

    /// Start the GitHub App bot server
    Serve {
        /// Port to listen on (defaults to $PORT env var, then 3000)
        #[arg(long)]
        port: Option<u16>,

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
            config,
            path,
        } => {
            let cfg = config::load(config.as_deref())?;
            let interactive_mode = !json && !ci && std::io::stdout().is_terminal();
            let findings = cli::run_check(cli::CheckOptions {
                staged: *staged,
                diff: diff.clone(),
                ci: *ci,
                llm: *llm,
                json: *json,
                path: path.clone(),
                config: &cfg,
                quiet: interactive_mode,
            })?;

            if interactive_mode {
                interactive::run(&findings, &cfg)?;
            } else {
                output::render(&findings, *json || *ci)?;
            }

            let should_exit = if cfg.behavior.strict {
                !findings.is_empty()
            } else {
                findings.has_blocking()
            };
            if should_exit && (*ci || *staged) {
                std::process::exit(1);
            }
        }
        Commands::Watch { path, config } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(cli::run_watch(path, config.as_deref()))?;
        }
        Commands::Serve { port, host } => {
            let port = port.unwrap_or_else(|| {
                std::env::var("PORT")
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(3000)
            });
            let config = bot::BotConfig {
                app_id: std::env::var("GITHUB_APP_ID")
                    .map_err(|_| anyhow::anyhow!("GITHUB_APP_ID required"))?,
                private_key: std::env::var("GITHUB_APP_PRIVATE_KEY")
                    .map_err(|_| anyhow::anyhow!("GITHUB_APP_PRIVATE_KEY required"))?,
                webhook_secret: std::env::var("GITHUB_WEBHOOK_SECRET")
                    .map_err(|_| anyhow::anyhow!("GITHUB_WEBHOOK_SECRET required"))?,
                host: host.clone(),
                port,
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
