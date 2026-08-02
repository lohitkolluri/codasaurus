//! Codasaurus — self-hosted GitHub App PR review agent.
//!
//! Binary commands: `serve`, `health`, `version`, `reset-password`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use codasaurus::bot;
use codasaurus::db;
use codasaurus::github_jwt;
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

    /// Health check for Docker / orchestrators (HTTP `/health` or `/health/ready`)
    Health {
        /// Port to check (defaults to `$PORT`, then 3000)
        #[arg(long)]
        port: Option<u16>,

        /// Host to check
        #[arg(long, default_value = "localhost")]
        host: String,

        /// Require readiness (DB ping ok). Use for Docker HEALTHCHECK after start-period.
        #[arg(long)]
        ready: bool,
    },

    /// Reset a local dashboard user's password (emergency recovery; no email flow)
    ResetPassword {
        /// Account email
        #[arg(long)]
        email: String,

        /// New password (min 10 characters)
        #[arg(long)]
        password: String,
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
            let database_url = match std::env::var("DATABASE_URL") {
                Ok(u) if !u.trim().is_empty() => u,
                _ if std::env::var_os("RENDER").is_some()
                    || std::env::var_os("RENDER_SERVICE_ID").is_some() =>
                {
                    anyhow::bail!(
                        "DATABASE_URL is required on Render. Set it to a Neon Free \
                         (or other always-free) Postgres URI — not Render free Postgres \
                         (that plan expires). See docs/run-for-free.md."
                    );
                }
                _ => "postgres://codasaurus:codasaurus@127.0.0.1:5432/codasaurus".into(),
            };

            let bot_config = std::env::var("GITHUB_APP_ID").ok().and_then(|_| {
                match github_jwt::require_private_key_from_env() {
                    Ok(key) => Some(bot::BotConfig {
                        app_id: std::env::var("GITHUB_APP_ID").unwrap(),
                        private_key: key,
                        webhook_secret: std::env::var("GITHUB_WEBHOOK_SECRET").unwrap_or_default(),
                        host: host.clone(),
                        port,
                    }),
                    Err(e) => {
                        eprintln!("  Warning: GITHUB_APP_ID set but private key missing: {e}");
                        eprintln!("  Running in dashboard-only mode (no GitHub bot)");
                        None
                    }
                }
            });

            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(serve::serve(host, port, &database_url, bot_config))?;
        }
        Commands::Version => {
            println!("codasaurus v{}", env!("CARGO_PKG_VERSION"));
        }
        Commands::ResetPassword { email, password } => {
            let email = email.trim().to_lowercase();
            if email.is_empty() {
                anyhow::bail!("--email is required");
            }
            if password.len() < 10 {
                anyhow::bail!("--password must be at least 10 characters");
            }
            let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://codasaurus:codasaurus@127.0.0.1:5432/codasaurus".into()
            });
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                let pool = db::create_pool(&database_url).await?;
                if !db::users::set_password(&pool, &email, password).await? {
                    anyhow::bail!("no local user found for {email}");
                }
                let _ = db::users::delete_sessions_for_email(&pool, &email).await;
                pool.close().await;
                println!("Password reset for {email}. Existing sessions cleared.");
                Ok::<(), anyhow::Error>(())
            })?;
        }
        Commands::Health { port, host, ready } => {
            let port = port
                .or_else(|| std::env::var("PORT").ok().and_then(|p| p.parse().ok()))
                .unwrap_or(3000);
            let path = if *ready { "/health/ready" } else { "/health" };
            let url = format!("http://{host}:{port}{path}");
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
