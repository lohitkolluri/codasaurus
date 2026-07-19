use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::AppState;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct DatabaseBody {
    pub provider: String, // "sqlite" | "postgres"
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
        .route("/database", post(setup_database))
        .route("/llm", get(get_llm_config).post(setup_llm))
        .route("/github/manifest-page", get(github_manifest_page))
        .route("/github/manifest-url", get(github_manifest_url))
        .route("/github", post(setup_github))
        .route("/github/callback", post(github_callback))
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
}

/// GET /api/v1/setup/status — check which setup steps have been completed.
async fn setup_status(
    State(state): State<AppState>,
) -> Result<Json<SetupStatus>, ApiError> {
    use db::config::get_config;

    let database = get_config(&state.pool, "database_provider")
        .await
        .ok()
        .flatten()
        .is_some()
        || std::env::var("DATABASE_URL").is_ok();

    // Check both DB config and env vars for GitHub. If GITHUB_APP_ID is set
    // in the environment, the bot is already configured at startup — no need
    // to create a new GitHub App through the manifest flow.
    let github = get_config(&state.pool, "github_app_id")
        .await
        .ok()
        .flatten()
        .is_some()
        || std::env::var("GITHUB_APP_ID").is_ok();

    let llm = get_config(&state.pool, "llm_provider").await.ok().flatten().is_some()
        || std::env::var("OPENROUTER_API_KEY").is_ok();

    let admin: bool = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE role = 'admin'",
    )
    .fetch_one(&state.pool.0)
    .await
    .map(|count| count > 0)
    .unwrap_or(false);

    let complete = database && llm && github && admin;

    Ok(Json(SetupStatus { database, llm, github, admin, complete }))
}

/// POST /api/v1/setup/database
async fn setup_database(
    State(state): State<AppState>,
    Json(body): Json<DatabaseBody>,
) -> Result<Json<SetupResponse>, ApiError> {
    let provider = body.provider.to_lowercase();

    match provider.as_str() {
        "sqlite" => {
            // SQLite is already connected via DATABASE_URL at startup.
            // Validate by running a query on the existing pool instead of
            // creating a new connection (which would use a relative path
            // from the CWD inside the container).
            sqlx::query_scalar::<_, i64>("SELECT 1")
                .fetch_one(&state.pool.0)
                .await
                .map_err(|e| ApiError::bad_request(format!("Test query failed: {}", e)))?;

            // Use the DATABASE_URL from env for the stored config so it
            // matches what the server actually uses at startup, falling
            // back to the body URL as a reasonable default.
            let db_url = std::env::var("DATABASE_URL").unwrap_or(body.url);

            // Store config
            db::config::set_config(&state.pool, "database_provider", "sqlite")
                .await?;
            db::config::set_config(&state.pool, "database_url", &db_url)
                .await?;

            Ok(Json(SetupResponse {
                status: "ok".into(),
                message: Some("SQLite connection verified".into()),
                test_passed: Some(true),
            }))
        }
        "postgres" => {
            if !body.url.starts_with("postgres://") && !body.url.starts_with("postgresql://") {
                return Err(ApiError::bad_request(
                    "Postgres URL must start with postgres:// or postgresql://",
                ));
            }

            let db_url = crate::db::normalize_database_url(&body.url);

            // Try connecting
            let test_pool = sqlx::PgPool::connect(&db_url)
                .await
                .map_err(|e| ApiError::bad_request(format!("Cannot connect: {}", e)))?;

            sqlx::query_scalar::<_, i64>("SELECT 1")
                .fetch_one(&test_pool)
                .await
                .map_err(|e| ApiError::bad_request(format!("Test query failed: {}", e)))?;

            test_pool.close().await;

            db::config::set_config(&state.pool, "database_provider", "postgres")
                .await?;
            db::config::set_config(&state.pool, "database_url", &db_url)
                .await?;

            Ok(Json(SetupResponse {
                status: "ok".into(),
                message: Some("Postgres connection verified".into()),
                test_passed: Some(true),
            }))
        }
        other => Err(ApiError::bad_request(format!(
            "Unsupported database provider: {}. Use 'sqlite' or 'postgres'.",
            other
        ))),
    }
}

/// GET /api/setup/llm — return current LLM config from DB + env vars.
async fn get_llm_config(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use db::config::get_config;

    let provider = get_config(&state.pool, "llm_provider")
        .await.ok().flatten()
        .or_else(|| {
            if std::env::var("OPENROUTER_API_KEY").is_ok() { Some("openrouter".into()) }
            else if std::env::var("CODASAURUS_BASE_URL").is_ok() { Some("custom".into()) }
            else { None }
        });

    let api_key = get_config(&state.pool, "openrouter_api_key")
        .await.ok().flatten()
        .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
        .map(|_| "••••••••".to_string()); // never expose full key

    let model = get_config(&state.pool, "llm_model")
        .await.ok().flatten()
        .or_else(|| std::env::var("CODASAURUS_MODEL").ok());

    let base_url = get_config(&state.pool, "llm_base_url")
        .await.ok().flatten()
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
    Query(params): Query<LlmTestParams>,
    Json(body): Json<LlmBody>,
) -> Result<Json<SetupResponse>, ApiError> {
    let base_url = body.base_url.as_deref().unwrap_or("");
    let api_key = body.api_key.as_deref().unwrap_or("");

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
        .map_err(|e| ApiError::internal(format!("Failed to build HTTP client: {}", e)))?;

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
            (format!("{}/v1/chat/completions", base), model.unwrap_or("qwen2.5-coder:7b"))
        }
        "custom" => {
            if base_url.is_empty() {
                return Ok(Some(false));
            }
            let base = base_url.trim_end_matches('/');
            (format!("{}/chat/completions", base), model.unwrap_or("default"))
        }
        _ => return Err(ApiError::bad_request(format!("Unknown provider: {}", provider))),
    };

    let mut req = client
        .post(&url)
        .json(&json!({
            "model": model_name,
            "messages": [{"role": "user", "content": "Say 'ok' and nothing else."}],
            "max_tokens": 10,
        }));

    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }

    match req.send().await {
        Ok(resp) => Ok(Some(resp.status().is_success())),
        Err(_) => Ok(Some(false)),
    }
}

/// Resolve the public-facing URL for the GitHub App manifest.
/// Checks DB config → `PUBLIC_URL` env var → auto-detects from request Host header.
async fn resolve_public_url(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> String {
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
    if let Some(host) = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
    {
        let scheme = if host.starts_with("localhost") || host.starts_with("127.") {
            "http"
        } else {
            "https"
        };
        return format!("{}://{}", scheme, host);
    }
    "http://localhost:3000".to_string()
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

    // Unique suffix so the GitHub App name doesn't collide
    let suffix: String = uuid::Uuid::new_v4().to_string().chars().take(4).collect();

    let manifest = json!({
        "name": format!("codasaurus-{}", suffix),
        "url": &public_url,
        "hook_attributes": {
            "url": "https://example.com/codasaurus-webhook",
            "active": false
        },
        "redirect_url": format!("{}/#/setup/github/callback", public_url),
        "callback_urls": [
            format!("{}/api/auth/github/callback", public_url)
        ],
        "setup_url": format!("{}/setup/complete", public_url),
        "setup_on_update": true,
        "public": false,
        "request_oauth_on_install": true,
        "default_permissions": {
            "pull_requests": "write",
            "checks": "write",
            "contents": "read",
            "issues": "read",
            "metadata": "read",
            "emails": "read"
        },
        "default_events": [
            "pull_request",
            "issue_comment",
            "push",
            "check_run",
            "check_suite"
        ]
    });

    let manifest_json = serde_json::to_string(&manifest)
        .map_err(|e| ApiError::internal(format!("Failed to serialize manifest: {}", e)))?;

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

    // Unique suffix so the GitHub App name doesn't collide
    let suffix: String = uuid::Uuid::new_v4().to_string().chars().take(4).collect();

    let manifest = json!({
        "name": format!("codasaurus-{}", suffix),
        "url": &public_url,
        "hook_attributes": {
            "url": "https://example.com/codasaurus-webhook",
            "active": false
        },
        "redirect_url": format!("{}/#/setup/github/callback", public_url),
        "callback_urls": [
            format!("{}/api/auth/github/callback", public_url)
        ],
        "setup_url": format!("{}/setup/complete", public_url),
        "setup_on_update": true,
        "public": false,
        "request_oauth_on_install": true,
        "default_permissions": {
            "pull_requests": "write",
            "checks": "write",
            "contents": "read",
            "issues": "read",
            "metadata": "read",
            "emails": "read"
        },
        "default_events": [
            "pull_request",
            "issue_comment",
            "push",
            "check_run",
            "check_suite"
        ]
    });

    Ok(Json(manifest))
}

/// POST /api/v1/setup/github
async fn setup_github(
    State(state): State<AppState>,
    Json(body): Json<GithubBody>,
) -> Result<Json<SetupResponse>, ApiError> {
    // Validate credentials by generating a JWT and hitting the GitHub API
    let now = chrono::Utc::now();
    let claims = jsonwebtoken::Header::default();
    let payload = jsonwebtoken::EncodingKey::from_rsa_pem(body.private_key.as_bytes())
        .map_err(|e| ApiError::bad_request(format!("Invalid private key PEM: {}", e)))?;

    let token = jsonwebtoken::encode(
        &claims,
        &json!({
            "iat": now.timestamp(),
            "exp": (now + chrono::Duration::minutes(5)).timestamp(),
            "iss": body.app_id,
        }),
        &payload,
    )
    .map_err(|e| ApiError::bad_request(format!("Failed to encode JWT: {}", e)))?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::internal(format!("Failed to build HTTP client: {}", e)))?;

    let resp = client
        .get("https://api.github.com/app")
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "codasaurus")
        .send()
        .await
        .map_err(|e| ApiError::bad_request(format!("GitHub API request failed: {}", e)))?;

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
    db::config::set_config(&state.pool, "github_webhook_secret", &body.webhook_secret)
        .await?;

    Ok(Json(SetupResponse {
        status: "ok".into(),
        message: Some("GitHub App credentials verified and saved".into()),
        test_passed: Some(test_passed),
    }))
}

/// POST /api/v1/setup/github/callback
async fn github_callback(
    State(state): State<AppState>,
    Json(body): Json<GithubCallbackBody>,
) -> Result<Json<GithubCallbackResponse>, ApiError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ApiError::internal(format!("Failed to build HTTP client: {}", e)))?;

    let resp = client
        .post(format!(
            "https://api.github.com/app-manifests/{}/conversions",
            body.code
        ))
        .header("User-Agent", "codasaurus")
        .send()
        .await
        .map_err(|e| ApiError::bad_request(format!("GitHub API request failed: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".into());
        return Err(ApiError::bad_request(format!(
            "GitHub manifest conversion failed (HTTP {}): {}",
            status, text
        )));
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ApiError::bad_request(format!("Invalid GitHub response: {}", e)))?;

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
    db::config::set_config(&state.pool, "github_webhook_secret", &webhook_secret)
        .await?;
    db::config::set_config(&state.pool, "github_app_name", &app_name).await?;
    db::config::set_config(&state.pool, "github_app_slug", &slug).await?;

    let install_url = format!("https://github.com/apps/{}/installations/new", slug);

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
    Json(body): Json<AdminBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !body.email.contains('@') {
        return Err(ApiError::bad_request("Invalid email address"));
    }
    if body.password.len() < 8 {
        return Err(ApiError::bad_request(
            "Password must be at least 8 characters",
        ));
    }

    db::users::create_user(&state.pool, &body.email, &body.password, "admin")
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.message().contains("UNIQUE") {
                    return ApiError::bad_request("An admin user already exists");
                }
            }
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(json!({ "status": "ok" })))
}
