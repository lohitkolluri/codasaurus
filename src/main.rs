//! Codasaurus — self-hosted GitHub App PR review agent.
//!
//! Binary commands: `serve`, `health`, `version`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use codasaurus::bot;
use codasaurus::serve;

#[derive(Parser)]
#[command(name = "codasaurus")]
#[command(about = "Self-hosted GitHub App PR review agent — Tier-1 detectors + optional BYOK LLM")]
#[command(version, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the GitHub App bot + dashboard server
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
        Commands::Serve { port, host } => {
            let port = port.unwrap_or_else(|| {
                std::env::var("PORT")
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(3000)
            });
            let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                format!(
                    "sqlite://{}?mode=rwc",
                    codasaurus::storage::data_dir()
                        .join("codasaurus.db")
                        .display()
                )
            });

            let bot_config =
                std::env::var("GITHUB_APP_ID")
                    .ok()
                    .and_then(|_| match resolve_private_key() {
                        Ok(key) => Some(bot::BotConfig {
                            app_id: std::env::var("GITHUB_APP_ID").unwrap(),
                            private_key: key,
                            webhook_secret: std::env::var("GITHUB_WEBHOOK_SECRET")
                                .unwrap_or_default(),
                            host: host.clone(),
                            port,
                        }),
                        Err(e) => {
                            eprintln!("  Warning: GITHUB_APP_ID set but private key missing: {e}");
                            eprintln!("  Running in dashboard-only mode (no GitHub bot)");
                            None
                        }
                    });

            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(serve::serve(host, port, &database_url, bot_config))?;
        }
        Commands::Version => {
            println!("codasaurus v{}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Health { port, host } => {
            let url = format!("http://{host}:{port}/health");
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
                    eprintln!("Health check error: {url} — {e}");
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
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| anyhow::anyhow!("Invalid base64 key: {e}"))?;
    String::from_utf8(decoded).map_err(|e| anyhow::anyhow!("Invalid UTF-8 in decoded key: {e}"))
}
