use anyhow::Result;
use clap::{Parser, Subcommand};
use codasaurus::bot;
use codasaurus::cli;
use codasaurus::config;
use codasaurus::interactive;
use codasaurus::output;
use codasaurus::serve;
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

    /// Run as a GitHub Action (reads GITHUB_EVENT_PATH and posts Check Run annotations)
    CheckRun {
        /// Path to the GitHub event payload (defaults to GITHUB_EVENT_PATH env var)
        #[arg(long, env = "GITHUB_EVENT_PATH")]
        event_path: Option<String>,

        /// Path to config file
        #[arg(long)]
        config: Option<String>,
    },

    /// Verify command — blast-radius analysis for changed symbols
    Verify {
        /// Check staged changes (default)
        #[arg(long)]
        staged: bool,

        /// Check diff against a git ref (e.g. --diff origin/main)
        #[arg(long)]
        diff: Option<String>,

        /// Run tests for impacted symbols
        #[arg(long)]
        run_tests: bool,

        /// Skip confirmation prompts for test execution
        #[arg(long)]
        force: bool,

        /// CI mode — JSON output, strict exit codes
        #[arg(long)]
        ci: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Path to config file
        #[arg(long)]
        config: Option<String>,

        /// Path to verify (file or directory). Default: staged changes
        path: Option<String>,
    },

    /// Print version information
    Version,

    /// Health check for Docker (checks HTTP /health endpoint)
    Health {
        /// Port to check
        #[arg(long, default_value = "3000")]
        port: u16,

        /// Host to check
        #[arg(long, default_value = "localhost")]
        host: String,
    },
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
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(cli::run_watch(path, config.as_deref()))?;
        }
        Commands::Serve { port, host } => {
            let port = port.unwrap_or_else(|| {
                std::env::var("PORT")
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(3000)
            });
            let database_url = std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| {
                    eprintln!("DATABASE_URL not set. PostgreSQL is required.");
                    std::process::exit(1);
                });

            // Bot config is optional — only load if env vars are set
            let bot_config = std::env::var("GITHUB_APP_ID").ok().and_then(|_| {
                match resolve_private_key() {
                    Ok(key) => {
                        Some(bot::BotConfig {
                            app_id: std::env::var("GITHUB_APP_ID").unwrap(),
                            private_key: key,
                            webhook_secret: std::env::var("GITHUB_WEBHOOK_SECRET").unwrap_or_default(),
                            host: host.clone(),
                            port,
                        })
                    }
                    Err(e) => {
                        eprintln!("  Warning: GITHUB_APP_ID set but private key missing: {}", e);
                        eprintln!("  Running in dashboard-only mode (no GitHub bot)");
                        None
                    }
                }
            });

            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(serve::serve(&host, port, &database_url, bot_config))?;
        }
        Commands::CheckRun { event_path, config } => {
            let _cfg = config::load(config.as_deref())?;
            codasaurus::action::run_check_run(event_path.clone())?;
        }
        Commands::Verify {
            staged,
            diff,
            run_tests,
            force,
            ci,
            json,
            config,
            path,
        } => {
            let cfg = config::load(config.as_deref())?;
            let opts = codasaurus::verify::VerifyOptions {
                staged: *staged,
                diff: diff.clone(),
                path: path.clone(),
                run_tests: *run_tests,
                force: *force,
                ci: *ci,
                json: *json,
                config: &cfg,
            };
            let report = codasaurus::verify::run_verify(opts)?;

            codasaurus::evidence::render_report(&report, *json || *ci)?;

            if report.verified && !report.fix_packets.is_empty() {
                // Verified but with warnings — still exit 0 unless running in CI
                if *ci {
                    eprintln!("Warning: {} fix packets generated but all verified.", report.fix_packets.len());
                }
            } else if !report.verified {
                if *ci || *force {
                    std::process::exit(1);
                }
            }
        }

        Commands::Version => {
            println!("codasaurus v{}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Health { port, host } => {
            let url = format!("http://{}:{}/health", host, port);
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .connect_timeout(std::time::Duration::from_secs(3))
                .build()
                .expect("reqwest client config is valid");
            match client.get(&url).send() {
                Ok(resp) if resp.status().is_success() => {
                    println!("Health check passed: {} {}", url, resp.status());
                    std::process::exit(0);
                }
                Ok(resp) => {
                    eprintln!("Health check failed: {} {}", url, resp.status());
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Health check error: {} — {}", url, e);
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}

/// Resolve the GitHub App private key from environment.
/// Tries GITHUB_APP_PRIVATE_KEY first (raw PEM), then GITHUB_APP_PRIVATE_KEY_B64
/// (base64url-encoded, no special chars — safe for PaaS .env files).
fn resolve_private_key() -> anyhow::Result<String> {
    if let Ok(key) = std::env::var("GITHUB_APP_PRIVATE_KEY") {
        return Ok(key);
    }
    let b64 = std::env::var("GITHUB_APP_PRIVATE_KEY_B64").map_err(|_| {
        anyhow::anyhow!("GITHUB_APP_PRIVATE_KEY or GITHUB_APP_PRIVATE_KEY_B64 required")
    })?;
    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE
        .decode(b64.as_bytes())
        .map_err(|e| anyhow::anyhow!("Invalid base64 key: {}", e))?;
    String::from_utf8(decoded).map_err(|e| anyhow::anyhow!("Invalid UTF-8 in decoded key: {}", e))
}
