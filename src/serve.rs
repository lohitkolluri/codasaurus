use anyhow::Result;
use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::path::Path;
use tokio::signal;
use tower_http::{
    limit::RequestBodyLimitLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::api::{self, AppState};
use crate::bot;
use crate::db;

const SYNC_KEYS: &[(&str, &str)] = &[
    ("GITHUB_APP_ID", "github_app_id"),
    // Private key is NOT synced here — the env var GITHUB_APP_PRIVATE_KEY_B64
    // uses base64 STANDARD encoding, while the DB stores raw PEM (from the
    // manifest flow or from decode-then-store). Syncing the base64 string
    // directly would corrupt the PEM field. The env var is read at runtime
    // as a fallback in resolve_bot_config().
    ("GITHUB_WEBHOOK_SECRET", "github_webhook_secret"),
    ("OPENROUTER_API_KEY", "openrouter_api_key"),
];

/// Start the unified server serving API + SPA + webhook on a single port.
pub async fn serve(
    host: &str,
    port: u16,
    database_url: &str,
    env_bot_config: Option<bot::BotConfig>,
) -> Result<()> {
    let pool = crate::db::create_pool(database_url).await?;

    // Sync env vars → DB config so the setup wizard detects them even
    // after ephemeral storage is wiped (Render free tier, Docker restarts).
    sync_env_to_db(&pool).await;

    println!("  Database connected (SQLite)");

    // Try loading bot config from DB (setup wizard may have stored it),
    // with env vars taking precedence.
    let bot_config = resolve_bot_config(&pool, host, port, env_bot_config).await;
    if bot_config.is_some() {
        println!("  GitHub bot configured");
    }

    let app = build_router(pool, bot_config);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    println!("  Codasaurus server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn build_router(pool: crate::db::DbPool, bot_config: Option<bot::BotConfig>) -> Router {
    let api = api::router().with_state(AppState { pool: pool.clone() });

    bot::set_config_pool(pool);

    if bot_config.as_ref().map(|c| c.webhook_secret.is_empty()).unwrap_or(true) {
        eprintln!("  ⚠ Webhook secret is empty — all GitHub events will return 401");
        eprintln!("  → Run the setup wizard to configure your GitHub App");
    } else {
        println!("  GitHub bot configured");
    }

    let bot_cfg = bot_config.unwrap_or_else(|| bot::BotConfig {
        app_id: String::new(),
        private_key: String::new(),
        webhook_secret: String::new(),
        host: "0.0.0.0".into(),
        port: 3000,
    });
    bot::set_bot_config(bot_cfg);

    // Direct POST route — no .nest(), no State/Extension, no state-laden routers.
    // GitHub sends POST to /webhook/ (with trailing slash). Register both variants
    // to avoid any Axum trailing-slash normalization issues.
    let webhook_handler = post(
        |headers: axum::http::HeaderMap, body: axum::body::Bytes| async move {
            bot::handle_webhook(headers, body).await
        },
    );

    let mut app = Router::new()
        .merge(api)
        .route("/health", get(health_handler))
        .route("/webhook", webhook_handler.clone())
        .route("/webhook/", webhook_handler);

    // SPA static file serving — acts as catch-all for unmatched routes
    let dist_path = Path::new("svelte-dashboard").join("dist");
    if dist_path.exists() {
        let serve_dir = ServeDir::new(&dist_path)
            .append_index_html_on_directories(true)
            .not_found_service(ServeFile::new(dist_path.join("index.html")));
        app = app.fallback_service(serve_dir);
    }

    // Layers applied to the final resolved router
    app = app
        .layer(TraceLayer::new_for_http())
        .layer(RequestBodyLimitLayer::new(25 * 1024 * 1024));

    app
}

/// Try to load bot credentials from environment variables first, then
/// the DB (stored by the setup wizard). If env vars gave us a partial
/// config (e.g. GITHUB_APP_ID set but GITHUB_WEBHOOK_SECRET missing),
/// fall back to DB-stored values for the missing fields.
async fn resolve_bot_config(
    pool: &db::DbPool,
    host: &str,
    port: u16,
    env_config: Option<bot::BotConfig>,
) -> Option<bot::BotConfig> {
    // If env vars already gave us a config, use it with DB fallbacks
    if let Some(cfg) = env_config {
        let webhook_secret = if cfg.webhook_secret.is_empty() {
            db::config::get_config(pool, "github_webhook_secret")
                .await
                .ok()
                .flatten()
                .unwrap_or_default()
        } else {
            cfg.webhook_secret
        };
        return Some(bot::BotConfig {
            webhook_secret,
            ..cfg
        });
    }

    // Try loading from DB (setup wizard stores here), with env var fallbacks
    let app_id = db::config::get_config(pool, "github_app_id")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("GITHUB_APP_ID").ok())?;

    let private_key = match db::config::get_config(pool, "github_private_key").await {
        Ok(Some(key)) => key,
        _ => {
            // Fall back to base64-encoded env var
            std::env::var("GITHUB_APP_PRIVATE_KEY_B64")
                .ok()
                .and_then(|b64| {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD
                        .decode(&b64)
                        .ok()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                })?
        }
    };

    let webhook_secret =
        db::config::get_config(pool, "github_webhook_secret")
            .await
            .ok()
            .flatten()
            .or_else(|| std::env::var("GITHUB_WEBHOOK_SECRET").ok())
            .unwrap_or_default();

    Some(bot::BotConfig {
        app_id,
        private_key,
        webhook_secret,
        host: host.to_string(),
        port,
    })
}

async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            println!("  Shutting down (Ctrl+C)...");
        }
        _ = terminate => {
            println!("  Shutting down (SIGTERM)...");
        }
    }
}

/// On startup, copy well-known env vars into the database config table.
/// This is idempotent — if the DB already has a value and the env var isn't
/// set, it leaves the DB value alone.  Purpose: Render's free tier has no
/// persistent disk, so when the container restarts from scratch we still get
/// the config the wizard wrote last time (because the *next* startup will
/// have the same env vars the user originally set).
async fn sync_env_to_db(pool: &db::DbPool) {
    for (env_key, config_key) in SYNC_KEYS {
        if let Ok(val) = std::env::var(env_key) {
            if !val.is_empty() {
                // Prefer an existing DB value over overwriting it
                // (env vars win on a fresh container, but once the wizard
                // stores something, let the DB value take over).
                if db::config::get_config(pool, config_key)
                    .await
                    .ok()
                    .flatten()
                    .is_none()
                {
                    let _ = db::config::set_config(pool, config_key, &val).await;
                }
            }
        }
    }
}
