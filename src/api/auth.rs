use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::db;

use super::errors::ApiError;
use super::rbac;
use super::AppState;

const SESSION_COOKIE: &str = "codasaurus_session";
const SESSION_MAX_AGE: &str = "604800"; // 7 days

const LOGIN_RATE_LIMIT: u32 = 10;
const LOGIN_RATE_WINDOW: Duration = Duration::from_secs(15 * 60);

/// Per-process login rate limiter: `{ip}/{email}` → (attempt count, window start).
///
/// Not shared across replicas and resets on restart. Fine for single-node /
/// Compose deploys. Behind a trusted reverse proxy, client IP comes from
/// `X-Forwarded-For` (first hop). For multi-instance production, put a shared
/// limiter in front (CDN / reverse proxy) — see `docs/configuration.md`.
static LOGIN_ATTEMPTS: LazyLock<Mutex<HashMap<String, (u32, Instant)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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

/// Secure cookies by default. Opt out with `CODASAURUS_INSECURE_COOKIES=1` or
/// when `PUBLIC_URL` is `http://localhost*` / `http://127.0.0.1*`.
fn cookie_should_be_secure() -> bool {
    if std::env::var("CODASAURUS_SECURE_COOKIES")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        return true;
    }
    if std::env::var("CODASAURUS_INSECURE_COOKIES")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        return false;
    }
    if let Ok(url) = std::env::var("PUBLIC_URL") {
        let u = url.to_ascii_lowercase();
        if u.starts_with("http://localhost") || u.starts_with("http://127.0.0.1") {
            return false;
        }
    }
    true
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

fn client_ip(headers: &axum::http::HeaderMap) -> String {
    if let Some(xff) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(first) = xff.split(',').next() {
            let t = first.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    if let Some(rip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let t = rip.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    "unknown".into()
}

fn check_auth_rate_limit(headers: &axum::http::HeaderMap, email: &str) -> Result<(), ApiError> {
    let key = format!("{}/{}", client_ip(headers), email.trim().to_lowercase());
    let mut map = LOGIN_ATTEMPTS
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    map.retain(|_, (_, started)| now.duration_since(*started) < LOGIN_RATE_WINDOW);
    let entry = map.entry(key).or_insert((0, now));
    if now.duration_since(entry.1) >= LOGIN_RATE_WINDOW {
        *entry = (0, now);
    }
    if entry.0 >= LOGIN_RATE_LIMIT {
        return Err(ApiError::too_many_requests(
            "Too many attempts. Try again in 15 minutes.",
        ));
    }
    entry.0 += 1;
    Ok(())
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
    headers: axum::http::HeaderMap,
    Json(body): Json<LoginBody>,
) -> Result<impl IntoResponse, ApiError> {
    check_auth_rate_limit(&headers, &body.email)?;

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
    let code_verifier = crate::oidc::take_pending(&state_param)
        .ok_or_else(|| ApiError::bad_request("invalid or expired OIDC state"))?;
    let cfg = crate::oidc::OidcConfig::from_env()
        .ok_or_else(|| ApiError::bad_request("OIDC is not configured"))?;
    let email = crate::oidc::exchange_code(&cfg, &code, &code_verifier)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let existing = db::users::get_user_by_email(&state.pool, &email)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let user = if let Some(u) = existing {
        u
    } else {
        match db::invites::accept_oidc(&state.pool, &email)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
        {
            Some(u) => u,
            None => {
                let open_join = std::env::var("OIDC_ALLOW_OPEN_JOIN")
                    .ok()
                    .is_some_and(|v| v == "1");
                if open_join {
                    db::users::upsert_oidc_user(&state.pool, &email, "viewer")
                        .await
                        .map_err(|e| ApiError::internal(e.to_string()))?
                } else {
                    return Err(ApiError::bad_request(
                        "No invite found for this email. Ask an admin for an invite link.",
                    ));
                }
            }
        }
    };
    let token = db::sessions::create_session(&state.pool, &email)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let _ = &user.role;
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
    headers: axum::http::HeaderMap,
    Path(token): Path<String>,
    Json(body): Json<AcceptInviteBody>,
) -> Result<impl IntoResponse, ApiError> {
    let invite = db::invites::get_pending_by_token(&state.pool, &token)
        .await?
        .ok_or_else(|| ApiError::not_found("Invite not found or expired"))?;

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

    check_auth_rate_limit(&headers, &email)?;

    if let Err(msg) = db::users::validate_password_policy(&body.password, &email) {
        return Err(ApiError::bad_request(msg));
    }

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
            if let sqlx::Error::Protocol(ref msg) = e {
                if msg.contains("Invite not found") {
                    return ApiError::bad_request(msg.clone());
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
