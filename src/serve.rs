use anyhow::Result;
use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::get,
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

/// Start the unified server serving API + SPA + webhook on a single port.
pub async fn serve(
    host: &str,
    port: u16,
    database_url: &str,
    env_bot_config: Option<bot::BotConfig>,
) -> Result<()> {
    let pool = crate::db::create_pool(database_url).await?;
    println!("  Database connected (SQLite)");

    // Try loading bot config from DB (setup wizard may have stored it),
    // with env vars taking precedence.
    let bot_config = resolve_bot_config(&pool, host, port, env_bot_config).await;
    if bot_config.is_some() {
        println!("  GitHub bot configured");
    } else {
        println!("  Starting in dashboard-only mode (no GitHub bot)");
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
    let state = AppState { pool };

    // API routes + health check (both use AppState)
    // api::router() nests sub-routers under full /api/v1/* paths,
    // so we merge at the root level.
    let mut app = Router::new()
        .merge(api::router())
        .route("/health", get(health_handler));

    // SPA static file serving — acts as catch-all for unmatched routes
    let dist_path = Path::new("svelte-dashboard").join("dist");
    if dist_path.exists() {
        let serve_dir = ServeDir::new(&dist_path)
            .append_index_html_on_directories(true)
            .not_found_service(ServeFile::new(dist_path.join("index.html")));
        app = app.fallback_service(serve_dir);
    }

    // Resolve shared state — converts Router<AppState> → Router<()>
    // Everything above this line shares the AppState.
    let mut app = app.with_state(state);

    // Bot webhook (has its own BotConfig state, already resolved by webhook_router)
    if let Some(config) = bot_config {
        app = app.nest("/webhook", bot::webhook_router(config));
    }

    // Layers applied to the final resolved router
    app = app
        .layer(TraceLayer::new_for_http())
        .layer(RequestBodyLimitLayer::new(10 * 1024 * 1024));

    app
}

/// Try to load bot credentials from the DB (stored by the setup wizard),
/// falling back to env vars if no env config was passed.
async fn resolve_bot_config(
    pool: &db::DbPool,
    host: &str,
    port: u16,
    env_config: Option<bot::BotConfig>,
) -> Option<bot::BotConfig> {
    // If env vars already gave us a config, use it directly
    if let Some(cfg) = env_config {
        return Some(cfg);
    }

    // Try loading from DB (setup wizard stores here)
    let app_id = db::config::get_config(pool, "github_app_id").await.ok()??;
    let private_key = db::config::get_config(pool, "github_private_key").await.ok()??;
    let webhook_secret =
        db::config::get_config(pool, "github_webhook_secret").await.ok()?.unwrap_or_default();

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
