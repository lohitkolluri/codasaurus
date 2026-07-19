use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::AppState;

const SESSION_COOKIE: &str = "codasaurus_session";
const SESSION_MAX_AGE: &str = "604800"; // 7 days

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

/// Extract the session token from the Cookie header.
fn extract_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in cookie.split(';') {
        let pair = pair.trim();
        if let Some(val) = pair.strip_prefix(&format!("{SESSION_COOKIE}=")) {
            return Some(val.to_string());
        }
    }
    None
}

fn set_cookie(token: &str) -> String {
    format!("{SESSION_COOKIE}={token}; HttpOnly; Path=/; Max-Age={SESSION_MAX_AGE}; SameSite=Lax")
}

fn clear_cookie() -> String {
    format!("{SESSION_COOKIE}=; HttpOnly; Path=/; Max-Age=0")
}

pub(crate) async fn require_session(
    pool: &crate::db::DbPool,
    headers: &axum::http::HeaderMap,
) -> Result<String, StatusCode> {
    let token = extract_token(headers).ok_or(StatusCode::UNAUTHORIZED)?;
    let email = db::sessions::get_session(pool, &token)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    Ok(email)
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
        .map_err(|e| ApiError::unauthorized(format!("Authentication failed: {e}")))?
        .ok_or_else(|| ApiError::unauthorized("Invalid email or password"))?;

    // Create session (non-fatal if fails — user still authenticated for this request)
    let cookie = if let Ok(token) = db::sessions::create_session(&state.pool, &user.email).await {
        set_cookie(&token)
    } else {
        clear_cookie()
    };

    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(LoginResponse {
            user: UserInfo {
                email: user.email,
                role: user.role,
            },
        }),
    ))
}

/// GET /api/v1/auth/me
async fn me(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Json<MeResponse> {
    // Try session-cookie auth first
    if let Some(token) = extract_token(&headers) {
        if let Ok(Some(email)) = db::sessions::get_session(&state.pool, &token).await {
            let role: String = sqlx::query_scalar("SELECT role FROM users WHERE email = ?")
                .bind(&email)
                .fetch_one(&state.pool.0)
                .await
                .unwrap_or_else(|_| "admin".into());

            return Json(MeResponse {
                authenticated: true,
                user: Some(UserInfo { email, role }),
            });
        }
    }

    // Fall back to Phase-1 behavior: check if any admin exists (used by setup wizard)
    let any_admin: bool = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(&state.pool.0)
        .await
        .map(|c: i64| c > 0)
        .unwrap_or(false);

    if any_admin {
        let email: String =
            sqlx::query_scalar("SELECT email FROM users WHERE role = 'admin' ORDER BY id LIMIT 1")
                .fetch_one(&state.pool.0)
                .await
                .unwrap_or_default();

        Json(MeResponse {
            authenticated: false,
            user: Some(UserInfo {
                email,
                role: "admin".into(),
            }),
        })
    } else {
        Json(MeResponse {
            authenticated: false,
            user: None,
        })
    }
}

/// POST /api/v1/auth/logout
async fn logout(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Some(token) = extract_token(&headers) {
        let _ = db::sessions::delete_session(&state.pool, &token).await;
    }
    (
        [(header::SET_COOKIE, clear_cookie())],
        Json(json!({ "status": "ok" })),
    )
}
