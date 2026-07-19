use axum::extract::State;
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
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/auth/login
async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> Result<Json<LoginResponse>, ApiError> {
    let user = db::users::verify_password(&state.pool, &body.email, &body.password)
        .await
        .map_err(|e| ApiError::unauthorized(format!("Authentication failed: {}", e)))?
        .ok_or_else(|| ApiError::unauthorized("Invalid email or password"))?;

    Ok(Json(LoginResponse {
        user: UserInfo {
            email: user.email,
            role: user.role,
        },
    }))
}

/// GET /api/v1/auth/me
///
/// Returns whether an admin user has been configured and who they are.
/// Phase 1: checks if any admin user exists in the DB.
async fn me(State(state): State<AppState>) -> Json<MeResponse> {
    let any_user: bool = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(&state.pool.0)
        .await
        .map(|count: i64| count > 0)
        .unwrap_or(false);

    if any_user {
        let email: String = sqlx::query_scalar(
            "SELECT email FROM users WHERE role = 'admin' ORDER BY id LIMIT 1",
        )
        .fetch_one(&state.pool.0)
        .await
        .unwrap_or_default();

        Json(MeResponse {
            authenticated: true,
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
async fn logout() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}
