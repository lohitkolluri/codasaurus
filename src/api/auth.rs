use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::rbac;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<String>,
    #[serde(default)]
    pub is_bootstrap: bool,
}

#[derive(Deserialize)]
pub struct AcceptInviteBody {
    pub email: Option<String>,
    pub password: String,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/me", get(me))
        .route("/logout", post(logout))
        .route("/oidc/status", get(oidc_status))
        .route("/oidc/login", get(oidc_login))
        .route("/oidc/callback", get(oidc_callback))
        .route("/invite/{token}", get(invite_info))
        .route("/invite/{token}/accept", post(accept_invite))
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

fn cookie_should_be_secure() -> bool {
    std::env::var("PUBLIC_URL")
        .ok()
        .filter(|u| u.starts_with("https://"))
        .is_some()
        || std::env::var("CODASAURUS_SECURE_COOKIES")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn set_cookie(token: &str) -> String {
    let mut cookie = format!(
        "{SESSION_COOKIE}={token}; HttpOnly; Path=/; Max-Age={SESSION_MAX_AGE}; SameSite=Lax"
    );
    if cookie_should_be_secure() {
        cookie.push_str("; Secure");
    }
    cookie
}

fn clear_cookie() -> String {
    let mut cookie = format!("{SESSION_COOKIE}=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax");
    if cookie_should_be_secure() {
        cookie.push_str("; Secure");
    }
    cookie
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

/// Axum middleware that requires a valid session cookie.
pub async fn auth_middleware(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: Next,
) -> Result<Response, ApiError> {
    require_session(&state.pool, req.headers())
        .await
        .map_err(|_| ApiError::unauthorized("Authentication required"))?;
    Ok(next.run(req).await)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/auth/login
async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> Result<impl IntoResponse, ApiError> {
    let user = db::users::verify_password(&state.pool, &body.email, &body.password)
        .await
        .map_err(|_| ApiError::unauthorized("Invalid email or password"))?
        .ok_or_else(|| ApiError::unauthorized("Invalid email or password"))?;

    let cookie = if let Ok(token) = db::sessions::create_session(&state.pool, &user.email).await {
        db::audit::log_event(
            &state.pool,
            "user.login",
            Some(&user.email),
            Some("user"),
            None,
        )
        .await;
        set_cookie(&token)
    } else {
        clear_cookie()
    };

    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(LoginResponse {
            user: UserInfo {
                email: user.email,
                role: rbac::normalize_role(&user.role).to_string(),
                auth_provider: None,
                is_bootstrap: user.is_bootstrap,
            },
        }),
    ))
}

/// GET /api/auth/me
async fn me(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Json<MeResponse> {
    if let Some(token) = extract_token(&headers) {
        if let Ok(Some(email)) = db::sessions::get_session(&state.pool, &token).await {
            let row: Option<(String, String, bool)> = {
                let prepared = state.pool.prepare_sql(
                    "SELECT role, auth_provider, is_bootstrap FROM users WHERE email = ?",
                );
                sqlx::query_as::<_, (String, String, bool)>(&prepared)
                    .bind(&email)
                    .fetch_optional(state.pool.as_pg())
                    .await
                    .ok()
                    .flatten()
            };
            let (role, auth_provider, is_bootstrap) =
                row.unwrap_or_else(|| ("viewer".into(), "local".into(), false));

            return Json(MeResponse {
                authenticated: true,
                user: Some(UserInfo {
                    email,
                    role: rbac::normalize_role(&role).to_string(),
                    auth_provider: Some(auth_provider),
                    is_bootstrap,
                }),
            });
        }
    }

    Json(MeResponse {
        authenticated: false,
        user: None,
    })
}

/// POST /api/auth/logout
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

async fn oidc_status() -> Json<serde_json::Value> {
    Json(json!({ "enabled": crate::oidc::OidcConfig::enabled() }))
}

async fn oidc_login() -> Result<impl IntoResponse, ApiError> {
    let cfg = crate::oidc::OidcConfig::from_env()
        .ok_or_else(|| ApiError::bad_request("OIDC is not configured"))?;
    let (url, _state) = crate::oidc::authorization_url(&cfg)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(axum::response::Redirect::temporary(&url))
}

#[derive(Deserialize)]
struct OidcCallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn oidc_callback(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<OidcCallbackParams>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = params.error {
        return Err(ApiError::bad_request(format!("OIDC error: {err}")));
    }
    let code = params
        .code
        .ok_or_else(|| ApiError::bad_request("missing code"))?;
    let state_param = params
        .state
        .ok_or_else(|| ApiError::bad_request("missing state"))?;
    if !crate::oidc::take_state(&state_param) {
        return Err(ApiError::bad_request("invalid or expired OIDC state"));
    }
    let cfg = crate::oidc::OidcConfig::from_env()
        .ok_or_else(|| ApiError::bad_request("OIDC is not configured"))?;
    let email = crate::oidc::exchange_code(&cfg, &code)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let existing = db::users::get_user_by_email(&state.pool, &email)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let role = if existing.is_some() {
        // Preserve role on re-login; do not consume invites for existing users.
        "viewer".to_string()
    } else {
        match db::invites::consume_for_oidc(&state.pool, &email)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
        {
            Some(r) => r,
            None => "viewer".to_string(),
        }
    };
    let _user = db::users::upsert_oidc_user(&state.pool, &email, &role)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let token = db::sessions::create_session(&state.pool, &email)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    db::audit::log_event(&state.pool, "user.login", Some(&email), Some("user"), None).await;
    let cookie = set_cookie(&token);
    Ok((
        StatusCode::SEE_OTHER,
        [
            (header::SET_COOKIE, cookie),
            (header::LOCATION, "/#/app/dashboard".into()),
        ],
    ))
}

/// GET /api/auth/invite/:token
async fn invite_info(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let invite = db::invites::get_pending_by_token(&state.pool, &token)
        .await?
        .ok_or_else(|| ApiError::not_found("Invite not found or expired"))?;
    Ok(Json(json!({
        "email": invite.email,
        "role": invite.role,
        "expires_at": invite.expires_at,
        "email_locked": invite.email.is_some(),
    })))
}

/// POST /api/auth/invite/:token/accept
async fn accept_invite(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(body): Json<AcceptInviteBody>,
) -> Result<impl IntoResponse, ApiError> {
    let invite = db::invites::get_pending_by_token(&state.pool, &token)
        .await?
        .ok_or_else(|| ApiError::not_found("Invite not found or expired"))?;

    if body.password.len() < 8 {
        return Err(ApiError::bad_request(
            "Password must be at least 8 characters",
        ));
    }

    let email = if let Some(locked) = &invite.email {
        if let Some(provided) = body.email.as_ref().map(|e| e.trim().to_lowercase()) {
            if !provided.is_empty() && provided != locked.to_lowercase() {
                return Err(ApiError::bad_request(
                    "This invite is locked to a different email",
                ));
            }
        }
        locked.clone()
    } else {
        let e = body
            .email
            .as_ref()
            .map(|e| e.trim().to_lowercase())
            .filter(|e| !e.is_empty())
            .ok_or_else(|| ApiError::bad_request("Email is required"))?;
        if !e.contains('@') {
            return Err(ApiError::bad_request("Invalid email address"));
        }
        e
    };

    if db::users::get_user_by_email(&state.pool, &email)
        .await?
        .is_some()
    {
        return Err(ApiError::bad_request(
            "A user with that email already exists",
        ));
    }

    let user = db::invites::accept_local(&state.pool, &invite, &email, &body.password)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.message().contains("UNIQUE") {
                    return ApiError::bad_request("A user with that email already exists");
                }
            }
            ApiError::internal(e.to_string())
        })?;

    let session = db::sessions::create_session(&state.pool, &user.email)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    db::audit::log_event(
        &state.pool,
        "user.accept",
        Some(&user.email),
        Some("invite"),
        Some(invite.id),
    )
    .await;

    Ok((
        [(header::SET_COOKIE, set_cookie(&session))],
        Json(json!({
            "user": {
                "email": user.email,
                "role": user.role,
            }
        })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_cookie_includes_httponly() {
        let c = set_cookie("abc");
        assert!(c.contains("HttpOnly"));
        assert!(c.contains("SameSite=Lax"));
    }
}
