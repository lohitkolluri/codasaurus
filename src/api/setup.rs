use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
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
        .route("/llm", post(setup_llm))
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

    let database = get_config(&state.pool, "database_provider").await.ok().flatten().is_some();
    let llm = get_config(&state.pool, "llm_provider").await.ok().flatten().is_some();
    let github = get_config(&state.pool, "github_app_id").await.ok().flatten().is_some();

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
            // Validate by trying to connect
            let test_pool = sqlx::SqlitePool::connect(&body.url)
                .await
                .map_err(|e| ApiError::bad_request(format!("Cannot connect: {}", e)))?;

            // Run a test query
            sqlx::query_scalar::<_, i64>("SELECT 1")
                .fetch_one(&test_pool)
                .await
                .map_err(|e| ApiError::bad_request(format!("Test query failed: {}", e)))?;

            test_pool.close().await;

            // Store config
            db::config::set_config(&state.pool, "database_provider", "sqlite")
                .await?;
            db::config::set_config(&state.pool, "database_url", &body.url)
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

            // Try connecting
            let test_pool = sqlx::PgPool::connect(&body.url)
                .await
                .map_err(|e| ApiError::bad_request(format!("Cannot connect: {}", e)))?;

            sqlx::query_scalar::<_, i64>("SELECT 1")
                .fetch_one(&test_pool)
                .await
                .map_err(|e| ApiError::bad_request(format!("Test query failed: {}", e)))?;

            test_pool.close().await;

            db::config::set_config(&state.pool, "database_provider", "postgres")
                .await?;
            db::config::set_config(&state.pool, "database_url", &body.url)
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

/// GET /api/setup/github/manifest-url
async fn github_manifest_url(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Use public URL from config, falling back to env var, then placeholder
    let public_url = db::config::get_config(&state.pool, "public_url")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("PUBLIC_URL").ok())
        .unwrap_or_else(|| "http://localhost:3000".to_string());

    // Auto-generate a webhook secret — the user doesn't need to set this manually
    let webhook_secret = uuid::Uuid::new_v4().to_string().replace('-', "");

    let manifest = json!({
        "name": "codasaurus",
        "url": &public_url,
        "hook_attributes": {
            "url": format!("{}/webhook", public_url),
            "secret": webhook_secret,
            "active": true
        },
        "redirect_url": format!("{}/setup/github/callback", public_url),
        "callback_urls": [
            format!("{}/api/auth/github/callback", public_url)
        ],
        "setup_url": format!("{}/setup/complete", public_url),
        "setup_on_update": true,
        "public": false,
        "request_oauth_on_install": true,
        "expire_user_tokens": true,
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
            "check_suite",
            "installation",
            "installation_repositories"
        ]
    });

    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::to_string(&manifest)
            .map_err(|e| ApiError::internal(format!("Failed to serialize manifest: {}", e)))?,
    );

    let url = format!("https://github.com/settings/apps/new?manifest={}", encoded);

    Ok(Json(json!({ "url": url })))
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
