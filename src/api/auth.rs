use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::AppState;

const SESSION_COOKIE: &str = "codasaurus_session";
const SESSION_MAX_AGE: &str = "604800"; // 7 days in seconds

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub user: UserInfo,
}

#[derive(Serialize)]
pub struct MeResponse {
    pub authenticated: bool,
    pub user: Option<UserInfo>,
}

#[derive(Serialize)]
pub struct UserInfo {
    pub email: String,
    pub role: String,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/me", get(me))
        .route("/logout", post(logout))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the session token from the `Cookie` header.
fn extract_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    // Cookie: key=val; key2=val2
    for pair in cookie.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix(&format!("{}=", SESSION_COOKIE)) {
            return Some(value.to_string());
        }
    }
    None
}

/// Build a `Set-Cookie` header value that sets the session cookie.
fn set_cookie(token: &str) -> String {
    format!(
        "{}={}; HttpOnly; Path=/; Max-Age={}; SameSite=Lax",
        SESSION_COOKIE, token, SESSION_MAX_AGE
    )
}

/// Build a `Set-Cookie` header that clears the session cookie.
fn clear_cookie() -> String {
    format!("{}=; HttpOnly; Path=/; Max-Age=0", SESSION_COOKIE)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/auth/login
async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> Result<impl IntoResponse, ApiError> {
    let user = db::users::verify_password(&state.pool, &body.email, &body.password)
        .await
        .map_err(|e| ApiError::unauthorized(format!("Authentication failed: {}", e)))?
        .ok_or_else(|| ApiError::unauthorized("Invalid email or password"))?;

    let token = db::sessions::create_session(&state.pool, &user.email)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let json = Json(LoginResponse {
        user: UserInfo {
            email: user.email,
            role: user.role,
        },
    });

    Ok((
        [(header::SET_COOKIE, set_cookie(&token))],
        json,
    ))
}

/// GET /api/v1/auth/me
async fn me(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<MeResponse>, ApiError> {
    let token = match extract_token(&headers) {
        Some(t) => t,
        None => {
            // No session — check if any admin exists for the wizard flow
            let has_admin: bool = sqlx::query_scalar(
                "SELECT COUNT(*) FROM users WHERE role = 'admin'",
            )
            .fetch_one(&state.pool.0)
            .await
            .map(|c: i64| c > 0)
            .unwrap_or(false);
            return Ok(Json(MeResponse {
                authenticated: false,
                user: if has_admin {
                    let email: String = sqlx::query_scalar(
                        "SELECT email FROM users WHERE role = 'admin' ORDER BY id LIMIT 1",
                    )
                    .fetch_one(&state.pool.0)
                    .await
                    .unwrap_or_default();
                    Some(UserInfo { email, role: "admin".into() })
                } else {
                    None
                },
            }));
        }
    };

    let email = match db::sessions::get_session(&state.pool, &token).await {
        Ok(Some(e)) => e,
        _ => {
            return Ok(Json(MeResponse {
                authenticated: false,
                user: None,
            }));
        }
    };

    let role: String = sqlx::query_scalar(
        "SELECT role FROM users WHERE email = ?",
    )
    .bind(&email)
    .fetch_one(&state.pool.0)
    .await
    .unwrap_or_else(|_| "admin".into());

    Ok(Json(MeResponse {
        authenticated: true,
        user: Some(UserInfo { email, role }),
    }))
}

/// POST /api/v1/auth/logout
async fn logout(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(token) = extract_token(&headers) {
        let _ = db::sessions::delete_session(&state.pool, &token).await;
    }

    Ok(([(header::SET_COOKIE, clear_cookie())], Json(json!({ "status": "ok" }))))
}
