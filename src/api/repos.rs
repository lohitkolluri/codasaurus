use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::bot::{github_api_headers, next_github_link, GITHUB_CLIENT};
use crate::db;
use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};

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

/// GET /api/repos
async fn list_repos(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let repos = db::repos::list_repos(&state.pool).await?;
    Ok(Json(json!(repos)))
}

/// POST /api/repos/sync — fetch all installations + repos from GitHub and
/// store them in the local database.  Returns the number of repos synced.
async fn sync_repos(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;
    use crate::db::config::get_config;

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
        .filter(|k| !k.trim().is_empty())
        .or_else(crate::github_jwt::resolve_private_key_from_env)
        .ok_or_else(|| ApiError::bad_request("No GitHub App private key configured"))?;

    let jwt = crate::github_jwt::create_app_jwt(&app_id, &private_key)
        .map_err(|e| ApiError::internal(format!("JWT error: {e}")))?;

    let client = GITHUB_CLIENT
        .as_ref()
        .ok_or_else(|| ApiError::internal("GitHub API client not available"))?;
    let jwt_auth = format!("Bearer {jwt}");
    let jwt_headers = github_api_headers(&jwt_auth)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let installations: Vec<serde_json::Value> = retry_async(
        &RetryConfig::api_default(),
        "sync_list_installations",
        &is_reqwest_error_retryable,
        || async {
            client
                .get("https://api.github.com/app/installations")
                .headers(jwt_headers.clone())
                .send()
                .await?
                .error_for_status()?
                .json()
                .await
                .map_err(Into::into)
        },
    )
    .await
    .map_err(|e| ApiError::bad_request(format!("GitHub API: {e}")))?;

    let mut total = 0usize;

    for inst in &installations {
        let inst_id = match inst["id"].as_i64() {
            Some(id) => id,
            None => continue,
        };

        let token_resp: serde_json::Value = retry_async(
            &RetryConfig::api_default(),
            "sync_installation_token",
            &is_reqwest_error_retryable,
            || async {
                client
                    .post(format!(
                        "https://api.github.com/app/installations/{inst_id}/access_tokens"
                    ))
                    .headers(jwt_headers.clone())
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await
                    .map_err(Into::into)
            },
        )
        .await
        .map_err(|e| ApiError::bad_request(format!("Token request: {e}")))?;

        let token = match token_resp["token"].as_str() {
            Some(t) => t.to_string(),
            None => continue,
        };
        let token_auth = format!("Bearer {token}");
        let token_headers = github_api_headers(&token_auth)
            .map_err(|e| ApiError::internal(e.to_string()))?;

        let mut all_repos: Vec<serde_json::Value> = Vec::new();
        let mut page_url: Option<String> =
            Some("https://api.github.com/installation/repositories?per_page=100".into());
        let mut pages = 0u32;

        while let Some(url) = page_url.take() {
            pages += 1;
            let resp = retry_async(
                &RetryConfig::api_default(),
                "sync_list_repos",
                &is_reqwest_error_retryable,
                || async {
                    client
                        .get(&url)
                        .headers(token_headers.clone())
                        .send()
                        .await
                        .map_err(Into::into)
                },
            )
            .await
            .map_err(|e| ApiError::bad_request(format!("Repos request: {e}")))?;

            page_url = next_github_link(resp.headers());

            let page_data: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| ApiError::bad_request(format!("Repos response: {e}")))?;

            if let Some(repos) = page_data["repositories"].as_array() {
                all_repos.extend(repos.iter().cloned());
            }

            if pages >= 10 || all_repos.len() >= 1000 {
                break;
            }
        }

        for repo in &all_repos {
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
                eprintln!("  [sync_repos] Failed to save repo: {e}");
            } else {
                total += 1;
            }
        }
    }

    Ok(Json(json!({ "status": "ok", "synced": total })))
}

/// GET /api/repos/:id
async fn get_repo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let repo = db::repos::get_repo(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Repo {id} not found")))?;
    Ok(Json(json!(repo)))
}

/// PUT /api/repos/:id
async fn update_repo(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<UpdateRepoBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;
    db::repos::update_repo(&state.pool, id, &body.config_json, body.active).await?;
    Ok(Json(json!({ "status": "ok" })))
}

/// DELETE /api/repos/:id
async fn delete_repo(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::rbac::require_maintainer(&state, &headers).await?;
    db::repos::delete_repo(&state.pool, id).await?;
    Ok(Json(json!({ "status": "ok" })))
}
