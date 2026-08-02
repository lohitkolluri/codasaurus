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
        .filter(|k| !k.trim().is_empty())
        .or_else(crate::github_jwt::resolve_private_key_from_env)
        .ok_or_else(|| ApiError::bad_request("No GitHub App private key configured"))?;

    let jwt = create_jwt(&app_id, &private_key)?;

    // List all installations
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ApiError::internal(format!("HTTP client: {e}")))?;

    let installations: Vec<serde_json::Value> = client
        .get("https://api.github.com/app/installations")
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus")
        .send()
        .await
        .map_err(|e| ApiError::bad_request(format!("GitHub API: {e}")))?
        .json()
        .await
        .map_err(|e| ApiError::bad_request(format!("Invalid response: {e}")))?;

    let mut total = 0usize;

    for inst in &installations {
        let inst_id = match inst["id"].as_i64() {
            Some(id) => id,
            None => continue,
        };

        // Get installation token
        let token_resp: serde_json::Value = client
            .post(format!(
                "https://api.github.com/app/installations/{inst_id}/access_tokens"
            ))
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "codasaurus")
            .send()
            .await
            .map_err(|e| ApiError::bad_request(format!("Token request: {e}")))?
            .json()
            .await
            .map_err(|e| ApiError::bad_request(format!("Token response: {e}")))?;

        let token = match token_resp["token"].as_str() {
            Some(t) => t.to_string(),
            None => continue,
        };

        // List repos for this installation — follow Link pagination.
        let mut all_repos: Vec<serde_json::Value> = Vec::new();
        let mut page_url: Option<String> =
            Some("https://api.github.com/installation/repositories?per_page=100".into());
        let mut pages = 0u32;

        while let Some(url) = page_url.take() {
            pages += 1;
            let resp = client
                .get(&url)
                .header("Authorization", format!("Bearer {token}"))
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "codasaurus")
                .send()
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

            // Guard: stop after 10 pages to prevent runaway loops
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

fn create_jwt(app_id: &str, private_key_pem: &str) -> Result<String, ApiError> {
    crate::github_jwt::create_app_jwt(app_id, private_key_pem)
        .map_err(|e| ApiError::internal(format!("JWT error: {e}")))
}

/// Parse GitHub `Link: <url>; rel="next"` header.
/// Only returns the URL when the host is exactly `api.github.com` (SSRF guard).
fn next_github_link(headers: &axum::http::HeaderMap) -> Option<String> {
    let link = headers.get(axum::http::header::LINK)?.to_str().ok()?;
    for part in link.split(',') {
        let part = part.trim();
        if !(part.contains("rel=\"next\"") || part.contains("rel='next'")) {
            continue;
        }
        let start = part.find('<')? + 1;
        let end = part.find('>')?;
        if start >= end {
            continue;
        }
        let candidate = &part[start..end];
        let Ok(url) = url::Url::parse(candidate) else {
            continue;
        };
        let Some(host) = url.host_str() else {
            continue;
        };
        if host.eq_ignore_ascii_case("api.github.com") {
            return Some(candidate.to_string());
        }
    }
    None
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
