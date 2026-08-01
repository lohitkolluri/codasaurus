//! Team members and invite-link management (owner-only).

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::db;

use super::errors::ApiError;
use super::rbac::{self, is_valid_role};
use super::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users))
        .route("/me/password", post(change_my_password))
        .route("/invites", get(list_invites).post(create_invite))
        .route("/invites/{id}", delete(revoke_invite))
        .route("/{id}/transfer-bootstrap", post(transfer_bootstrap))
        .route("/{id}", patch(update_user).delete(remove_user))
}

fn public_base_url() -> String {
    std::env::var("PUBLIC_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| "http://localhost:3000".into())
        .trim_end_matches('/')
        .to_string()
}

fn invite_url(raw_token: &str) -> String {
    format!("{}/#/invite/{}", public_base_url(), raw_token)
}

#[derive(Deserialize)]
pub struct CreateInviteBody {
    pub email: Option<String>,
    pub role: String,
}

#[derive(Deserialize)]
pub struct UpdateUserBody {
    pub role: String,
}

#[derive(Deserialize)]
pub struct ChangePasswordBody {
    pub current_password: String,
    pub new_password: String,
}

/// GET /api/users
async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Any signed-in member can see the roster; mutations stay owner-only.
    rbac::require_role(&state.pool, &headers, rbac::MinRole::Viewer).await?;
    let users = db::users::list_users(&state.pool).await?;
    let items: Vec<serde_json::Value> = users
        .into_iter()
        .map(|u| {
            json!({
                "id": u.id,
                "email": u.email,
                "role": rbac::normalize_role(&u.role),
                "auth_provider": u.auth_provider,
                "is_bootstrap": u.is_bootstrap,
                "created_at": u.created_at,
            })
        })
        .collect();
    Ok(Json(json!({ "users": items })))
}

/// POST /api/users/invites
async fn create_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateInviteBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = rbac::require_owner(&state, &headers).await?;
    if !is_valid_role(&body.role) {
        return Err(ApiError::bad_request(
            "role must be owner, maintainer, or viewer",
        ));
    }
    let email = body
        .email
        .as_ref()
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty());
    if let Some(ref e) = email {
        if !e.contains('@') {
            return Err(ApiError::bad_request("Invalid email address"));
        }
        if db::users::get_user_by_email(&state.pool, e)
            .await?
            .is_some()
        {
            return Err(ApiError::bad_request("A user with that email already exists"));
        }
    }

    let (invite, raw) = db::invites::create_invite(
        &state.pool,
        email.as_deref(),
        &body.role,
        &actor.email,
        7,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    db::audit::log_event(
        &state.pool,
        "user.invite",
        Some(&actor.email),
        Some("invite"),
        Some(invite.id),
    )
    .await;

    Ok(Json(json!({
        "id": invite.id,
        "email": invite.email,
        "role": invite.role,
        "expires_at": invite.expires_at,
        "url": invite_url(&raw),
        "token": raw,
    })))
}

/// GET /api/users/invites
async fn list_invites(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    rbac::require_owner(&state, &headers).await?;
    let invites = db::invites::list_pending(&state.pool).await?;
    let items: Vec<serde_json::Value> = invites
        .into_iter()
        .map(|i| {
            json!({
                "id": i.id,
                "email": i.email,
                "role": i.role,
                "created_by": i.created_by,
                "expires_at": i.expires_at,
                "created_at": i.created_at,
            })
        })
        .collect();
    Ok(Json(json!({ "invites": items })))
}

/// DELETE /api/users/invites/:id
async fn revoke_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = rbac::require_owner(&state, &headers).await?;
    let ok = db::invites::revoke(&state.pool, id).await?;
    if !ok {
        return Err(ApiError::not_found("Invite not found"));
    }
    db::audit::log_event(
        &state.pool,
        "user.invite_revoke",
        Some(&actor.email),
        Some("invite"),
        Some(id),
    )
    .await;
    Ok(Json(json!({ "status": "ok" })))
}

/// PATCH /api/users/:id
async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<UpdateUserBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = rbac::require_owner(&state, &headers).await?;
    if !is_valid_role(&body.role) {
        return Err(ApiError::bad_request(
            "role must be owner, maintainer, or viewer",
        ));
    }
    let target = db::users::get_user_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("User not found"))?;
    if target.is_bootstrap && body.role != "owner" {
        return Err(ApiError::bad_request(
            "Cannot demote the bootstrap owner. Transfer bootstrap to another owner first.",
        ));
    }
    let was_owner = rbac::role_rank(&target.role) >= rbac::role_rank("owner");
    let will_be_owner = body.role == "owner";
    if was_owner && !will_be_owner {
        let owners = db::users::owner_count(&state.pool).await?;
        if owners <= 1 {
            return Err(ApiError::bad_request(
                "Cannot demote the last owner",
            ));
        }
    }
    let updated = db::users::update_user_role(&state.pool, id, &body.role).await?;
    db::audit::log_event(
        &state.pool,
        "user.role_change",
        Some(&actor.email),
        Some("user"),
        Some(id),
    )
    .await;
    Ok(Json(json!({
        "id": updated.id,
        "email": updated.email,
        "role": updated.role,
        "is_bootstrap": updated.is_bootstrap,
    })))
}

/// DELETE /api/users/:id
async fn remove_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = rbac::require_owner(&state, &headers).await?;
    let target = db::users::get_user_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("User not found"))?;
    if target.is_bootstrap {
        return Err(ApiError::bad_request(
            "Cannot remove the bootstrap owner. Transfer bootstrap to another owner first.",
        ));
    }
    if rbac::role_rank(&target.role) >= rbac::role_rank("owner") {
        let owners = db::users::owner_count(&state.pool).await?;
        if owners <= 1 {
            return Err(ApiError::bad_request("Cannot remove the last owner"));
        }
    }
    if target.email == actor.email {
        let owners = db::users::owner_count(&state.pool).await?;
        if owners <= 1 {
            return Err(ApiError::bad_request("Cannot remove the last owner"));
        }
    }
    db::users::delete_sessions_for_email(&state.pool, &target.email).await?;
    let ok = db::users::delete_user(&state.pool, id).await?;
    if !ok {
        return Err(ApiError::not_found("User not found"));
    }
    db::audit::log_event(
        &state.pool,
        "user.remove",
        Some(&actor.email),
        Some("user"),
        Some(id),
    )
    .await;
    Ok(Json(json!({ "status": "ok" })))
}

/// POST /api/users/:id/transfer-bootstrap — hand the instance superuser flag to another owner.
async fn transfer_bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = rbac::require_owner(&state, &headers).await?;
    let actor_user = db::users::get_user_by_email(&state.pool, &actor.email)
        .await?
        .ok_or_else(|| ApiError::unauthorized("Authentication required"))?;
    if !actor_user.is_bootstrap {
        return Err(ApiError::forbidden(
            "Only the bootstrap owner can transfer that role",
        ));
    }
    let target = db::users::get_user_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("User not found"))?;
    if target.id == actor_user.id {
        return Err(ApiError::bad_request("Already the bootstrap owner"));
    }
    if rbac::normalize_role(&target.role) != "owner" {
        return Err(ApiError::bad_request(
            "Bootstrap can only transfer to another owner",
        ));
    }
    let updated = db::users::transfer_bootstrap(&state.pool, id).await?;
    db::audit::log_event(
        &state.pool,
        "user.bootstrap_transfer",
        Some(&actor.email),
        Some("user"),
        Some(id),
    )
    .await;
    Ok(Json(json!({
        "id": updated.id,
        "email": updated.email,
        "role": updated.role,
        "is_bootstrap": updated.is_bootstrap,
    })))
}

/// POST /api/users/me/password
async fn change_my_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ChangePasswordBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = rbac::current_user(&state.pool, &headers).await?;
    if body.new_password.len() < 8 {
        return Err(ApiError::bad_request(
            "Password must be at least 8 characters",
        ));
    }
    let user = db::users::get_user_by_email(&state.pool, &actor.email)
        .await?
        .ok_or_else(|| ApiError::unauthorized("Authentication required"))?;
    if user.password_hash.is_empty() {
        return Err(ApiError::bad_request(
            "OIDC accounts cannot set a local password here",
        ));
    }
    let ok = db::users::verify_password(&state.pool, &actor.email, &body.current_password)
        .await?
        .is_some();
    if !ok {
        return Err(ApiError::unauthorized("Current password is incorrect"));
    }
    db::users::set_password(&state.pool, &actor.email, &body.new_password).await?;
    Ok(Json(json!({ "status": "ok" })))
}
