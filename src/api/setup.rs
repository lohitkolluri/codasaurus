use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bot;
use crate::db;
use crate::github_jwt;
use crate::ssrf;

use super::errors::ApiError;
use super::AppState;

/// Reject mutating setup routes once an owner exists, unless the caller is an owner.
async fn require_setup_open_or_admin(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), ApiError> {
    let owner_exists = db::users::owner_exists(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if !owner_exists {
        return Ok(());
    }
    super::rbac::require_owner(state, headers).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct DatabaseBody {
    pub provider: String, // "postgres"
    pub url: String,
}

#[derive(Deserialize)]
pub struct LlmBody {
    pub provider: String, // "openrouter" | "ollama" | "custom"
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Deserialize)]
pub struct LlmTestParams {
    pub test: Option<bool>,
}

#[derive(Serialize)]
pub struct SetupResponse {
    pub status: String,
    pub message: Option<String>,
    pub test_passed: Option<bool>,
}

#[derive(Deserialize)]
pub struct GithubBody {
    pub app_id: String,
    pub private_key: String,
    pub webhook_secret: String,
}

#[derive(Deserialize)]
pub struct GithubCallbackBody {
    pub code: String,
}

#[derive(Deserialize)]
pub struct GithubCallbackQuery {
    pub code: Option<String>,
}

#[derive(Serialize)]
pub struct GithubCallbackResponse {
    pub status: String,
    pub app_id: String,
    pub app_name: String,
    pub slug: String,
    pub install_url: String,
}

#[derive(Deserialize)]
pub struct AdminBody {
    pub email: String,
    pub password: String,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/status", get(setup_status))
        .route("/database", get(get_database_status).post(setup_database))
        .route("/llm", get(get_llm_config).post(setup_llm))
        .route("/github/manifest-page", get(github_manifest_page))
        .route("/github/manifest-url", get(github_manifest_url))
        .route("/github", post(setup_github))
        .route(
            "/github/callback",
            get(github_callback_page).post(github_callback),
        )
        .route("/admin", post(setup_admin))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SetupStatus {
    pub database: bool,
    pub llm: bool,
    pub github: bool,
    pub admin: bool,
    pub complete: bool,
    /// Present when a GitHub App slug is known — used by the complete screen before login.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_install_url: Option<String>,
}

/// GET /api/v1/setup/status — check which setup steps have been completed.
async fn setup_status(State(state): State<AppState>) -> Result<Json<SetupStatus>, ApiError> {
    use db::config::get_config;

    // Serving implies Postgres is already connected; also honor wizard / env markers.
    let database = true;

    // Check both DB config and env vars for GitHub. If GITHUB_APP_ID is set
    // in the environment, the bot is already configured at startup — no need
    // to create a new GitHub App through the manifest flow.
    let github = get_config(&state.pool, "github_app_id")
        .await
        .ok()
        .flatten()
        .is_some()
        || std::env::var("GITHUB_APP_ID").is_ok();

    let llm = get_config(&state.pool, "llm_provider")
        .await
        .ok()
        .flatten()
        .is_some()
        || std::env::var("OPENROUTER_API_KEY").is_ok();

    let admin: bool = crate::db::db_scalar!(
        &state.pool,
        i64,
        "SELECT COUNT(*) FROM users WHERE role IN ('owner', 'admin')"
    )
    .map(|count| count > 0)
    .unwrap_or(false);

    let complete = database && llm && github && admin;

    let github_install_url = get_config(&state.pool, "github_app_slug")
        .await
        .ok()
        .flatten()
        .map(|slug| format!("https://github.com/apps/{slug}/installations/new"));

    Ok(Json(SetupStatus {
        database,
        llm,
        github,
        admin,
        complete,
        github_install_url,
    }))
}

/// GET /api/v1/setup/database — live Postgres connection summary for the wizard.
async fn get_database_status(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .pool
        .ping()
        .await
        .map_err(|e| ApiError::internal(format!("Postgres ping failed: {e}")))?;

    let (version,): (String,) = sqlx::query_as("SELECT version()")
        .fetch_one(state.pool.as_pg())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let short_version = version
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");

    let url = std::env::var("DATABASE_URL").unwrap_or_default();
    let (host, database) = redact_pg_url(&url);

    Ok(Json(json!({
        "connected": true,
        "provider": "postgres",
        "host": host,
        "database": database,
        "server_version": short_version,
        "hint": "Runtime always uses DATABASE_URL from the Codasaurus process. Compose starts Postgres for you.",
    })))
}

fn redact_pg_url(raw: &str) -> (String, String) {
    let Ok(parsed) = url::Url::parse(raw) else {
        return ("(unknown)".into(), "(unknown)".into());
    };
    let host = parsed
        .host_str()
        .map(|h| {
            if let Some(port) = parsed.port() {
                format!("{h}:{port}")
            } else {
                h.to_string()
            }
        })
        .unwrap_or_else(|| "(socket)".into());
    let database = parsed.path().trim_start_matches('/').to_string();
    let database = if database.is_empty() {
        "(default)".into()
    } else {
        database
    };
    (host, database)
}

/// POST /api/v1/setup/database
async fn setup_database(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<DatabaseBody>,
) -> Result<Json<SetupResponse>, ApiError> {
    require_setup_open_or_admin(&state, &headers).await?;
    let provider = body.provider.to_lowercase();

    match provider.as_str() {
        "postgres" | "" => {
            // Always verify the live pool first (Compose / env boots).
            state
                .pool
                .ping()
                .await
                .map_err(|e| ApiError::bad_request(format!("Live Postgres ping failed: {e}")))?;

            let mut message = "Connected to PostgreSQL".to_string();

            if !body.url.is_empty() {
                if !body.url.starts_with("postgres://") && !body.url.starts_with("postgresql://") {
                    return Err(ApiError::bad_request(
                        "Postgres URL must start with postgres:// or postgresql://",
                    ));
                }
                let db_url = crate::db::normalize_database_url(&body.url);
                // SSRF: reject private/metadata hosts before connecting.
                // Loopback (localhost / 127.0.0.1) is allowed for local setup.
                ssrf::validate_postgres_url_resolved(&db_url)
                    .await
                    .map_err(ApiError::bad_request)?;
                let test_pool = sqlx::PgPool::connect(&db_url)
                    .await
                    .map_err(|e| ApiError::bad_request(format!("Cannot connect: {e}")))?;
                sqlx::query_scalar::<_, i32>("SELECT 1")
                    .fetch_one(&test_pool)
                    .await
                    .map_err(|e| ApiError::bad_request(format!("Test query failed: {e}")))?;
                test_pool.close().await;
                // Do not persist credentials in app_config — use DATABASE_URL env.
                message =
                    "Postgres URL validated (not stored; set DATABASE_URL for the server)".into();
            }

            db::config::set_config(&state.pool, "database_provider", "postgres").await?;

            Ok(Json(SetupResponse {
                status: "ok".into(),
                message: Some(message),
                test_passed: Some(true),
            }))
        }
        other => Err(ApiError::bad_request(format!(
            "Unsupported database provider: {other}. Codasaurus requires PostgreSQL."
        ))),
    }
}

/// GET /api/setup/llm — return current LLM config from DB + env vars.
async fn get_llm_config(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use db::config::get_config;

    let provider = get_config(&state.pool, "llm_provider")
        .await
        .ok()
        .flatten()
        .or_else(|| {
            if std::env::var("OPENROUTER_API_KEY").is_ok() {
                Some("openrouter".into())
            } else if std::env::var("CODASAURUS_BASE_URL").is_ok() {
                Some("custom".into())
            } else {
                None
            }
        });

    let api_key = get_config(&state.pool, "openrouter_api_key")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
        .map(|_| "••••••••".to_string()); // never expose full key

    let model = get_config(&state.pool, "llm_model")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("CODASAURUS_MODEL").ok());

    let base_url = get_config(&state.pool, "llm_base_url")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("CODASAURUS_BASE_URL").ok());

    Ok(Json(json!({
        "provider": provider,
        "api_key": api_key,
        "model": model,
        "base_url": base_url,
    })))
}

/// POST /api/v1/setup/llm?test=true
async fn setup_llm(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(params): Query<LlmTestParams>,
    Json(body): Json<LlmBody>,
) -> Result<Json<SetupResponse>, ApiError> {
    require_setup_open_or_admin(&state, &headers).await?;
    let base_url = body.base_url.as_deref().unwrap_or("");
    let api_key = body.api_key.as_deref().unwrap_or("");

    // SSRF guard for user-supplied endpoints
    if !base_url.is_empty() {
        let allow_loopback = body.provider == "ollama";
        ssrf::validate_llm_base_url_resolved(base_url, allow_loopback)
            .await
            .map_err(ApiError::bad_request)?;
    }

    // Always store the config
    db::config::set_config(&state.pool, "llm_provider", &body.provider).await?;
    if !api_key.is_empty() {
        db::config::set_config(&state.pool, "openrouter_api_key", api_key).await?;
    }
    if let Some(model) = &body.model {
        db::config::set_config(&state.pool, "llm_model", model).await?;
    }
    if !base_url.is_empty() {
        db::config::set_config(&state.pool, "llm_base_url", base_url).await?;
    }

    // If ?test=true, send a lightweight probe to the LLM endpoint
    let test_passed = if params.test.unwrap_or(false) {
        test_llm_connection(&body.provider, api_key, body.model.as_deref(), base_url).await?
    } else {
        None
    };

    Ok(Json(SetupResponse {
        status: "ok".into(),
        message: if test_passed.is_some() {
            Some("LLM configuration saved".into())
        } else {
            None
        },
        test_passed,
    }))
}

/// Try a basic chat-completion probe against the provider.
async fn test_llm_connection(
    provider: &str,
    api_key: &str,
    model: Option<&str>,
    base_url: &str,
) -> Result<Option<bool>, ApiError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| ApiError::internal(format!("Failed to build HTTP client: {e}")))?;

    let (url, model_name) = match provider {
        "openrouter" => (
            "https://openrouter.ai/api/v1/chat/completions".to_string(),
            model.unwrap_or("qwen/qwen3-coder:free"),
        ),
        "ollama" => {
            let base = if base_url.is_empty() {
                "http://localhost:11434"
            } else {
                base_url.trim_end_matches('/')
            };
            (
                format!("{base}/v1/chat/completions"),
                model.unwrap_or("qwen2.5-coder:7b"),
            )
        }
        "custom" => {
            if base_url.is_empty() {
                return Ok(Some(false));
            }
            ssrf::validate_llm_base_url_resolved(base_url, false)
                .await
                .map_err(ApiError::bad_request)?;
            let base = base_url.trim_end_matches('/');
            (
                format!("{base}/chat/completions"),
                model.unwrap_or("default"),
            )
        }
        "disabled" => return Ok(Some(true)),
        _ => {
            return Err(ApiError::bad_request(format!(
                "Unknown provider: {provider}"
            )))
        }
    };

    let mut req = client.post(&url).json(&json!({
        "model": model_name,
        "messages": [{"role": "user", "content": "Say 'ok' and nothing else."}],
        "max_tokens": 10,
    }));

    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {api_key}"));
    }

    match req.send().await {
        Ok(resp) => Ok(Some(resp.status().is_success())),
        Err(_) => Ok(Some(false)),
    }
}

/// Resolve the public-facing URL for the GitHub App manifest.
/// Checks DB config → `PUBLIC_URL` env var → auto-detects from request Host header.
async fn resolve_public_url(state: &AppState, headers: &axum::http::HeaderMap) -> String {
    if let Some(url) = db::config::get_config(&state.pool, "public_url")
        .await
        .ok()
        .flatten()
    {
        return url;
    }
    if let Ok(url) = std::env::var("PUBLIC_URL") {
        return url;
    }
    // Auto-detect from request Host header
    if let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) {
        let scheme = if host.starts_with("localhost") || host.starts_with("127.") {
            "http"
        } else {
            "https"
        };
        return format!("{scheme}://{host}");
    }
    "http://localhost:3000".to_string()
}

/// Build the GitHub App manifest JSON shared by the HTML page and JSON endpoints.
fn build_manifest(public_url: &str) -> serde_json::Value {
    let suffix: String = uuid::Uuid::new_v4().to_string().chars().take(4).collect();
    json!({
        "name": format!("codasaurus-{}", suffix),
        "url": public_url,
        "hook_attributes": {
            "url": format!("{}/webhook/", public_url),
            "active": true
        },
        "redirect_url": format!("{}/api/setup/github/callback", public_url),
        "callback_urls": [
            format!("{}/api/auth/github/callback", public_url)
        ],
        "setup_url": format!("{}/#/setup/complete", public_url),
        "setup_on_update": true,
        "public": false,
        "request_oauth_on_install": false,
        "default_permissions": {
            "pull_requests": "write",
            "checks": "write",
            "contents": "write",
            "issues": "read",
            "metadata": "read",
            "emails": "read",
            "reactions": "read"
        },
        "default_events": [
            "pull_request",
            "issue_comment",
            "push",
            "check_run",
            "check_suite",
            "reaction"
        ]
    })
}

/// GET /api/setup/github/manifest-page — auto-submitting HTML form that
/// POSTs the manifest to GitHub. This is the officially documented flow:
/// https://docs.github.com/en/apps/sharing-github-apps/registering-a-github-app-from-a-manifest
async fn github_manifest_page(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    use axum::http::{header, StatusCode};
    use axum::response::Html;

    let public_url = resolve_public_url(&state, &headers).await;
    let manifest = build_manifest(&public_url);

    let manifest_json = serde_json::to_string(&manifest)
        .map_err(|e| ApiError::internal(format!("Failed to serialize manifest: {e}")))?;

    // HTML page that auto-submits the manifest form to GitHub.
    // This matches GitHub's documented POST form flow exactly.
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Redirecting to GitHub…</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
           display: flex; justify-content: center; align-items: center;
           height: 100vh; margin: 0; background: #f6f8fa; color: #1f2328; }}
    .card {{ text-align: center; padding: 40px; background: #fff;
             border-radius: 8px; box-shadow: 0 1px 3px rgba(0,0,0,.12); }}
    .spinner {{ border: 3px solid #d0d7de; border-top-color: #0969da;
                border-radius: 50%; width: 24px; height: 24px;
                animation: spin .8s linear infinite; margin: 16px auto; }}
    @keyframes spin {{ to {{ transform: rotate(360deg); }} }}
  </style>
</head>
<body>
  <div class="card">
    <p style="font-size:14px;margin:0 0 8px">Redirecting to GitHub to create your app…</p>
    <div class="spinner"></div>
    <form id="f" method="post" action="https://github.com/settings/apps/new">
      <input type="hidden" name="manifest" value='{}'>
    </form>
  </div>
  <script>document.getElementById("f").submit();</script>
</body>
</html>"#,
        // Escape single quotes for the HTML attribute
        manifest_json.replace('\'', "&#39;")
    );

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(html),
    ))
}

/// GET /api/setup/github/manifest — return the manifest JSON for programmatic use.
async fn github_manifest_url(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let public_url = resolve_public_url(&state, &headers).await;
    Ok(Json(build_manifest(&public_url)))
}

/// POST /api/v1/setup/github
async fn setup_github(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<GithubBody>,
) -> Result<Json<SetupResponse>, ApiError> {
    require_setup_open_or_admin(&state, &headers).await?;

    // Validate credentials by generating an RS256 JWT and hitting the GitHub API
    let token = github_jwt::create_app_jwt(&body.app_id, &body.private_key)
        .map_err(|e| ApiError::bad_request(format!("Invalid GitHub App credentials: {e}")))?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::internal(format!("Failed to build HTTP client: {e}")))?;

    let resp = client
        .get("https://api.github.com/app")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "codasaurus")
        .send()
        .await
        .map_err(|e| ApiError::bad_request(format!("GitHub API request failed: {e}")))?;

    let test_passed = resp.status().is_success();
    if !test_passed {
        return Err(ApiError::bad_request(format!(
            "GitHub API rejected credentials (HTTP {})",
            resp.status()
        )));
    }

    // Store credentials
    db::config::set_config(&state.pool, "github_app_id", &body.app_id).await?;
    db::config::set_config(&state.pool, "github_private_key", &body.private_key).await?;
    db::config::set_config(&state.pool, "github_webhook_secret", &body.webhook_secret).await?;

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    bot::set_bot_config(bot::BotConfig {
        app_id: body.app_id.clone(),
        private_key: body.private_key.clone(),
        webhook_secret: body.webhook_secret.clone(),
        host,
        port,
    });
    bot::ensure_review_worker(state.pool.clone());

    Ok(Json(SetupResponse {
        status: "ok".into(),
        message: Some("GitHub App credentials verified and saved".into()),
        test_passed: Some(test_passed),
    }))
}

/// GET /api/v1/setup/github/callback — lightweight HTML page that exchanges
/// the manifest code with GitHub, then redirects to the SPA entry point.
/// This avoids SPA routing issues (hash vs path) because the exchange happens
/// in a plain page before handing control back to the SPA.
async fn github_callback_page(
    Query(params): Query<GithubCallbackQuery>,
) -> impl axum::response::IntoResponse {
    use axum::http::{header, StatusCode};
    use axum::response::Html;

    let code = match params.code {
        Some(ref c) if !c.is_empty() => c.clone(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                Html(
                    r#"<html><body style="font:14px sans-serif;padding:40px">
                        <p>Missing authorization code from GitHub.</p>
                        <a href="/#/setup/github">Back to setup</a>
                    </body></html>"#
                        .into(),
                ),
            );
        }
    };

    // Reject anything that isn't a safe OAuth code charset to prevent XSS when
    // embedding into the HTML/JS bootstrap page.
    if !code
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '/' | '=' | '+'))
        || code.len() > 512
    {
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            Html(
                r#"<html><body style="font:14px sans-serif;padding:40px">
                    <p>Invalid authorization code.</p>
                    <a href="/#/setup/github">Back to setup</a>
                </body></html>"#
                    .into(),
            ),
        );
    }

    // Pass code via JSON serialization so it cannot break out of a JS string.
    let code_json = serde_json::to_string(&code).unwrap_or_else(|_| "\"\"".into());

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Completing GitHub App setup…</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
           display: flex; justify-content: center; align-items: center;
           height: 100vh; margin: 0; background: #f6f8fa; color: #1f2328; }}
    .card {{ text-align: center; padding: 40px; background: #fff;
             border-radius: 8px; box-shadow: 0 1px 3px rgba(0,0,0,.12); max-width: 480px; }}
    .spinner {{ border: 3px solid #d0d7de; border-top-color: #0969da;
                border-radius: 50%; width: 24px; height: 24px;
                animation: spin .8s linear infinite; margin: 16px auto; }}
    @keyframes spin {{ to {{ transform: rotate(360deg); }} }}
    .error {{ color: #cf222e; }}
    .success {{ color: #1a7f37; }}
    button {{ margin-top: 16px; padding: 8px 20px; font-size: 14px;
              border-radius: 6px; border: 1px solid #d0d7de; background: #f6f8fa; cursor: pointer; }}
  </style>
</head>
<body>
  <div class="card">
    <p id="status">Exchanging code with GitHub…</p>
    <div class="spinner" id="spinner"></div>
    <div id="result"></div>
  </div>
  <script>
    (async () => {{
      try {{
        const code = {code_json};
        const r = await fetch('/api/setup/github/callback', {{
          method: 'POST',
          headers: {{ 'Content-Type': 'application/json' }},
          body: JSON.stringify({{"code": code}})
        }});
        const data = await r.json();
        if (!r.ok) throw new Error(data.error || 'Request failed');
        document.getElementById('spinner').style.display = 'none';
        document.getElementById('status').textContent = 'GitHub App created and credentials saved!';
        document.getElementById('status').className = 'success';
        const btn = document.createElement('button');
        btn.textContent = 'Return to Setup';
        btn.onclick = () => {{ window.location.href = '/#/setup/github'; }};
        document.getElementById('result').appendChild(btn);
        // Also try to update the opener tab
        if (window.opener && !window.opener.closed) {{
          window.opener.location.href = '/#/setup/github';
        }}
      }} catch (err) {{
        document.getElementById('spinner').style.display = 'none';
        document.getElementById('status').textContent = 'Error: ' + err.message;
        document.getElementById('status').className = 'error';
        const btn = document.createElement('button');
        btn.textContent = 'Back to Setup';
        btn.onclick = () => {{ window.location.href = '/#/setup/github'; }};
        document.getElementById('result').appendChild(btn);
      }}
    }})();
  </script>
</body>
</html>"#
    );

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(html),
    )
}

/// POST /api/v1/setup/github/callback
async fn github_callback(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<GithubCallbackBody>,
) -> Result<Json<GithubCallbackResponse>, ApiError> {
    require_setup_open_or_admin(&state, &headers).await?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ApiError::internal(format!("Failed to build HTTP client: {e}")))?;

    let resp = client
        .post(format!(
            "https://api.github.com/app-manifests/{}/conversions",
            body.code
        ))
        .header("User-Agent", "codasaurus")
        .send()
        .await
        .map_err(|e| ApiError::bad_request(format!("GitHub API request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_else(|_| "unknown error".into());
        return Err(ApiError::bad_request(format!(
            "GitHub manifest conversion failed (HTTP {status}): {text}"
        )));
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ApiError::bad_request(format!("Invalid GitHub response: {e}")))?;

    let app_id = data["id"]
        .as_i64()
        .ok_or_else(|| ApiError::bad_request("Missing 'id' in GitHub response"))?
        .to_string();
    let pem = data["pem"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing 'pem' in GitHub response"))?
        .to_string();
    let webhook_secret = data["webhook_secret"]
        .as_str()
        .ok_or_else(|| ApiError::bad_request("Missing 'webhook_secret' in GitHub response"))?
        .to_string();
    let slug = data["slug"]
        .as_str()
        .or_else(|| data["name"].as_str())
        .unwrap_or("codasaurus")
        .to_string();
    let app_name = data["name"].as_str().unwrap_or(&slug).to_string();

    db::config::set_config(&state.pool, "github_app_id", &app_id).await?;
    db::config::set_config(&state.pool, "github_private_key", &pem).await?;
    db::config::set_config(&state.pool, "github_webhook_secret", &webhook_secret).await?;
    db::config::set_config(&state.pool, "github_app_name", &app_name).await?;
    db::config::set_config(&state.pool, "github_app_slug", &slug).await?;

    // Update the in-memory bot config so the webhook handler picks up the
    // new credentials immediately without requiring a server restart.
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    bot::set_bot_config(bot::BotConfig {
        app_id: app_id.clone(),
        private_key: pem,
        webhook_secret: webhook_secret.clone(),
        host,
        port,
    });
    // Wizard-first installs often boot without credentials; start workers now if not yet running.
    bot::ensure_review_worker(state.pool.clone());

    let install_url = format!("https://github.com/apps/{slug}/installations/new");

    Ok(Json(GithubCallbackResponse {
        status: "ok".into(),
        app_id,
        app_name,
        slug,
        install_url,
    }))
}

/// POST /api/v1/setup/admin
async fn setup_admin(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<AdminBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Only allow first owner without auth; subsequent creates require owner.
    let owner_exists = db::users::owner_exists(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if owner_exists {
        super::rbac::require_owner(&state, &headers).await?;
    }

    if !body.email.contains('@') {
        return Err(ApiError::bad_request("Invalid email address"));
    }
    if let Err(msg) = db::users::validate_password_policy(&body.password, &body.email) {
        return Err(ApiError::bad_request(msg));
    }

    let result = if owner_exists {
        // Additional owners via wizard (rare) — not bootstrap.
        db::users::create_user(&state.pool, &body.email, &body.password, "owner").await
    } else {
        // Day-zero onboarding account = instance bootstrap / superuser.
        db::users::create_bootstrap_owner(&state.pool, &body.email, &body.password).await
    };
    result.map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.message().contains("UNIQUE") {
                return ApiError::bad_request("A user with that email already exists");
            }
        }
        ApiError::internal(e.to_string())
    })?;

    Ok(Json(json!({ "status": "ok" })))
}
