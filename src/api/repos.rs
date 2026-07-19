use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::AppState;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct UpdateRepoBody {
    pub config_json: String,
    pub active: bool,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_repos))
        .route("/sync", post(sync_repos))
        .route("/{id}", get(get_repo))
        .route("/{id}", put(update_repo))
        .route("/{id}", delete(delete_repo))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/repos
async fn list_repos(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let repos = db::repos::list_repos(&state.pool).await?;
    Ok(Json(json!(repos)))
}

/// POST /api/v1/repos/sync — fetch all installations + repos from GitHub and
/// store them in the local database.  Returns the number of repos synced.
async fn sync_repos(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    use crate::db::config::get_config;

    // Read GitHub credentials from DB config
    let app_id = get_config(&state.pool, "github_app_id")
        .await
        .ok()
        .flatten()
        .or_else(|| std::env::var("GITHUB_APP_ID").ok())
        .ok_or_else(|| ApiError::bad_request("No GitHub App configured"))?;
    let private_key = get_config(&state.pool, "github_private_key")
        .await
        .ok()
        .flatten()
        .or_else(|| {
            std::env::var("GITHUB_APP_PRIVATE_KEY_B64")
                .ok()
                .and_then(|b64| base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b64).ok())
                .and_then(|bytes| String::from_utf8(bytes).ok())
        })
        .ok_or_else(|| ApiError::bad_request("No GitHub App private key configured"))?;

    let jwt = create_jwt(&app_id, &private_key)?;

    // List all installations
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ApiError::internal(format!("HTTP client: {}", e)))?;

    let installations: Vec<serde_json::Value> = client
        .get("https://api.github.com/app/installations")
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus")
        .send()
        .await
        .map_err(|e| ApiError::bad_request(format!("GitHub API: {}", e)))?
        .json()
        .await
        .map_err(|e| ApiError::bad_request(format!("Invalid response: {}", e)))?;

    let mut total = 0usize;

    for inst in &installations {
        let inst_id = match inst["id"].as_i64() {
            Some(id) => id,
            None => continue,
        };

        // Get installation token
        let token_resp: serde_json::Value = client
            .post(format!(
                "https://api.github.com/app/installations/{}/access_tokens",
                inst_id
            ))
            .header("Authorization", format!("Bearer {}", jwt))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "codasaurus")
            .send()
            .await
            .map_err(|e| ApiError::bad_request(format!("Token request: {}", e)))?
            .json()
            .await
            .map_err(|e| ApiError::bad_request(format!("Token response: {}", e)))?;

        let token = match token_resp["token"].as_str() {
            Some(t) => t.to_string(),
            None => continue,
        };

        // List repos for this installation
        let repos_resp: serde_json::Value = client
            .get("https://api.github.com/installation/repositories")
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "codasaurus")
            .send()
            .await
            .map_err(|e| ApiError::bad_request(format!("Repos request: {}", e)))?
            .json()
            .await
            .map_err(|e| ApiError::bad_request(format!("Repos response: {}", e)))?;

        let repos = repos_resp["repositories"].as_array().cloned().unwrap_or_default();

        for repo in &repos {
            let github_id = repo["id"].as_i64();
            let full_name = match repo["full_name"].as_str() {
                Some(n) => n.to_string(),
                None => continue,
            };
            let (owner, name) = match full_name.split_once('/') {
                Some((o, n)) => (o.to_string(), n.to_string()),
                None => continue,
            };
            let default_branch = repo["default_branch"].as_str().map(|s| s.to_string());
            let private = repo["private"].as_bool().unwrap_or(false);

            if let Err(e) = db::repos::create_repo(
                &state.pool,
                &db::models::RepoCreate {
                    github_id,
                    full_name,
                    owner,
                    name,
                    default_branch,
                    installation_id: inst_id,
                    private,
                },
            )
            .await
            {
                eprintln!("  [sync_repos] Failed to save repo: {}", e);
            } else {
                total += 1;
            }
        }
    }

    Ok(Json(json!({ "status": "ok", "synced": total })))
}

fn create_jwt(app_id: &str, private_key_pem: &str) -> Result<String, ApiError> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let payload = serde_json::json!({
        "iat": now.saturating_sub(60),
        "exp": now + 600,
        "iss": app_id,
    });

    let key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
        .map_err(|e| ApiError::internal(format!("Invalid PEM: {}", e)))?;

    encode(&Header::new(Algorithm::RS256), &payload, &key)
        .map_err(|e| ApiError::internal(format!("JWT error: {}", e)))
}

/// GET /api/v1/repos/:id
async fn get_repo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let repo = db::repos::get_repo(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Repo {} not found", id)))?;
    Ok(Json(json!(repo)))
}

/// PUT /api/v1/repos/:id
async fn update_repo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateRepoBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    db::repos::update_repo(&state.pool, id, &body.config_json, body.active).await?;
    Ok(Json(json!({ "status": "ok" })))
}

/// DELETE /api/v1/repos/:id
async fn delete_repo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    db::repos::delete_repo(&state.pool, id).await?;
    Ok(Json(json!({ "status": "ok" })))
}
