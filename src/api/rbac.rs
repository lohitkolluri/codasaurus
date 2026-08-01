//! Role-based access helpers for the dashboard API.
//!
//! Hierarchy: owner > maintainer > viewer.

use axum::http::HeaderMap;

use crate::db;
use crate::db::models::UserView;

use super::auth;
use super::errors::ApiError;
use super::AppState;

/// Minimum role required for an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinRole {
    Viewer,
    Maintainer,
    Owner,
}

impl MinRole {
    fn rank(self) -> u8 {
        match self {
            MinRole::Viewer => 1,
            MinRole::Maintainer => 2,
            MinRole::Owner => 3,
        }
    }
}

pub fn role_rank(role: &str) -> u8 {
    match role {
        "owner" | "admin" => 3, // admin = legacy alias during migration
        "maintainer" => 2,
        "viewer" => 1,
        _ => 0,
    }
}

pub fn is_valid_role(role: &str) -> bool {
    matches!(role, "owner" | "maintainer" | "viewer")
}

pub fn normalize_role(role: &str) -> &str {
    match role {
        "admin" => "owner",
        r if is_valid_role(r) => r,
        _ => "viewer",
    }
}

/// Load the authenticated user (email + role) from the session cookie.
pub async fn current_user(
    pool: &db::DbPool,
    headers: &HeaderMap,
) -> Result<UserView, ApiError> {
    let email = auth::require_session(pool, headers)
        .await
        .map_err(|_| ApiError::unauthorized("Authentication required"))?;
    let user = db::users::get_user_by_email(pool, &email)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::unauthorized("Authentication required"))?;
    Ok(UserView {
        email: user.email,
        role: normalize_role(&user.role).to_string(),
        is_bootstrap: user.is_bootstrap,
    })
}

/// Require the session user to have at least `min` role.
pub async fn require_role(
    pool: &db::DbPool,
    headers: &HeaderMap,
    min: MinRole,
) -> Result<UserView, ApiError> {
    let user = current_user(pool, headers).await?;
    if role_rank(&user.role) < min.rank() {
        return Err(ApiError::forbidden(
            "Insufficient permissions for this action",
        ));
    }
    Ok(user)
}

pub async fn require_owner(state: &AppState, headers: &HeaderMap) -> Result<UserView, ApiError> {
    require_role(&state.pool, headers, MinRole::Owner).await
}

pub async fn require_maintainer(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<UserView, ApiError> {
    require_role(&state.pool, headers, MinRole::Maintainer).await
}
