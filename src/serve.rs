use anyhow::Result;
use axum::{
    error_handling::HandleErrorLayer,
    extract::Request,
    http::{header, HeaderValue, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    BoxError, Router,
};
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use subtle::ConstantTimeEq;
use tokio::signal;
use tower::{ServiceBuilder, ServiceExt};
use tower_http::{
    compression::CompressionLayer,
    limit::RequestBodyLimitLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::api::{self, AppState};
use crate::bot;
use crate::db;

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .try_init();
}

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
    init_tracing();

    let pool = crate::db::create_pool(database_url).await?;

    // Sync env vars → DB config so the setup wizard detects them even
    // after ephemeral storage is wiped (Render free tier, Docker restarts).
    sync_env_to_db(&pool).await;

    tracing::info!("Database connected (PostgreSQL)");
    println!("  Database connected (PostgreSQL)");

    // Mark setup database step complete for Compose / env-based boots.
    // Do not persist DATABASE_URL credentials into app_config.
    let _ = crate::db::config::set_config(&pool, "database_provider", "postgres").await;

    // Try loading bot config from DB (setup wizard may have stored it),
    // with env vars taking precedence.
    let bot_config = resolve_bot_config(&pool, host, port, env_bot_config).await;
    if bot_config.is_some() {
        println!("  GitHub bot configured");
    }

    // Apply offline mode before accepting traffic so slash commands honor the DB flag.
    {
        let db_off = crate::db::config::get_config(&pool, "offline_mode")
            .await
            .ok()
            .flatten();
        let offline = crate::bot::offline::offline_mode_from_env_and_db(db_off.as_deref());
        crate::registry::set_offline_mode(offline);
        if offline {
            tracing::info!("offline_mode enabled at boot — registry/OSV fail-closed");
            println!("  Offline mode: enabled (fail-closed)");
        }
    }

    let app = build_router(pool.clone(), bot_config);

    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    println!("  Codasaurus server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Drain the pool so Postgres sees clean disconnects on deploy/restart.
    pool.close().await;
    tracing::info!("database pool closed");
    Ok(())
}

fn build_router(pool: crate::db::DbPool, bot_config: Option<bot::BotConfig>) -> Router {
    let state = AppState { pool: pool.clone() };
    let api = api::build_router(state);

    bot::set_config_pool(pool.clone());

    if bot_config
        .as_ref()
        .map(|c| c.webhook_secret.is_empty())
        .unwrap_or(true)
    {
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
    bot::set_bot_config(bot_cfg.clone());
    // Always start workers so wizard-first deploys drain the queue after GitHub setup
    // without requiring a process restart. Workers reload credentials per job.
    bot::ensure_review_worker(pool);
    println!("  Review queue worker started");

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
        .route("/health/ready", get(ready_handler))
        .route("/webhook", webhook_handler.clone())
        .route("/webhook/", webhook_handler);

    // Metrics only when an auth token is configured (Bearer required).
    if std::env::var("CODASAURUS_METRICS_TOKEN")
        .map(|t| !t.is_empty())
        .unwrap_or(false)
    {
        app = app.route("/metrics", get(metrics_handler));
        println!("  /metrics enabled (CODASAURUS_METRICS_TOKEN set)");
    }

    // SPA static file serving — catch-all, but never mask unknown /api routes as HTML.
    let dist_path = Path::new("svelte-dashboard").join("dist");
    if dist_path.exists() {
        let serve_dir = ServeDir::new(&dist_path)
            .append_index_html_on_directories(true)
            .not_found_service(ServeFile::new(dist_path.join("index.html")));
        app = app.fallback(move |req: Request| {
            let serve_dir = serve_dir.clone();
            async move {
                if req.uri().path().starts_with("/api") {
                    return (
                        StatusCode::NOT_FOUND,
                        axum::Json(serde_json::json!({ "error": "not_found" })),
                    )
                        .into_response();
                }
                match serve_dir.oneshot(req).await {
                    Ok(res) => res.into_response(),
                    Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }
        });
    } else {
        app = app.fallback(|uri: Uri| async move {
            if uri.path().starts_with("/api") {
                (
                    StatusCode::NOT_FOUND,
                    axum::Json(serde_json::json!({ "error": "not_found" })),
                )
                    .into_response()
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        });
    }

    // Layers: last applied = outermost.
    app = app
        .layer(middleware::from_fn(security_headers_middleware))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|err: BoxError| async move {
                    if err.is::<tower::timeout::error::Elapsed>() {
                        StatusCode::REQUEST_TIMEOUT
                    } else {
                        tracing::error!(error = %err, "unhandled service error");
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                }))
                .timeout(Duration::from_secs(120)),
        )
        .layer(RequestBodyLimitLayer::new(25 * 1024 * 1024));

    app
}

async fn security_headers_middleware(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; connect-src 'self'; frame-ancestors 'none'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    // HSTS only when serving (or advertised) over HTTPS — local HTTP stays usable.
    let hsts = std::env::var("CODASAURUS_HSTS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
        || std::env::var("PUBLIC_URL")
            .map(|u| u.starts_with("https://"))
            .unwrap_or(false);
    if hsts {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
    headers.insert(
        header::HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("geolocation=(), microphone=(), camera=()"),
    );
    res
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

    let webhook_secret = db::config::get_config(pool, "github_webhook_secret")
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
    health_response(false).await
}

/// Readiness: requires writable data_dir **and** a successful DB ping.
async fn ready_handler() -> impl IntoResponse {
    health_response(true).await
}

async fn health_response(require_db: bool) -> impl IntoResponse {
    // Liveness-first for free PaaS + serverless Postgres cold starts:
    // `/health` stays 200 when only DB is waking; `/health/ready` fails closed.
    let db_ok = if let Some(pool) = crate::bot::CONFIG_POOL.get() {
        matches!(
            tokio::time::timeout(std::time::Duration::from_secs(2), pool.ping()).await,
            Ok(Ok(()))
        )
    } else {
        false
    };
    let data_dir = crate::storage::data_dir();
    let data_dir_ok = std::fs::create_dir_all(&data_dir).is_ok();
    let status = if !data_dir_ok || (require_db && !db_ok) {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    let mut offline = crate::bot::offline::offline_mode_from_env_and_db(None);
    let mut llm_disabled = false;
    let mut has_llm_endpoint = false;
    if let Some(pool) = crate::bot::CONFIG_POOL.get() {
        let db_off = crate::db::config::get_config(pool, "offline_mode")
            .await
            .ok()
            .flatten();
        offline = crate::bot::offline::offline_mode_from_env_and_db(db_off.as_deref());
        if let Ok(Some(p)) = crate::db::config::get_config(pool, "llm_provider").await {
            llm_disabled = p.eq_ignore_ascii_case("disabled");
        }
        if let Ok(Some(url)) = crate::db::config::get_config(pool, "llm_base_url").await {
            has_llm_endpoint = !url.trim().is_empty();
        }
        if let Ok(Some(key)) = crate::db::config::get_config(pool, "openrouter_api_key").await {
            if !key.trim().is_empty() {
                has_llm_endpoint = true;
            }
        }
    }
    let profile =
        crate::bot::offline::resolve_egress_profile(offline, llm_disabled, has_llm_endpoint);
    let llm_allowed = !llm_disabled && has_llm_endpoint && !offline;
    let egress = crate::bot::offline::health_json(profile, offline, llm_allowed);

    (
        status,
        axum::Json(serde_json::json!({
            "status": if db_ok && data_dir_ok { "ok" } else if data_dir_ok { "degraded" } else { "unhealthy" },
            "db": db_ok,
            "data_dir": data_dir_ok,
            "ready": db_ok && data_dir_ok,
            "version": env!("CARGO_PKG_VERSION"),
            "egress_profile": egress["egress_profile"],
            "offline_mode": egress["offline_mode"],
            "network": egress["network"],
            "note": egress["note"],
        })),
    )
}

async fn metrics_handler(headers: axum::http::HeaderMap) -> impl IntoResponse {
    let Ok(expected) = std::env::var("CODASAURUS_METRICS_TOKEN") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if expected.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }

    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        })
        .is_some_and(|token| bool::from(token.as_bytes().ct_eq(expected.as_bytes())));

    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if let Some(pool) = crate::bot::CONFIG_POOL.get() {
        crate::metrics::refresh_from_db(pool).await;
    }
    let body = crate::metrics::render_prometheus();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
        .into_response()
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
