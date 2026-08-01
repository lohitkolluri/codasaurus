//! Invite-link storage (hashed tokens only).

use argon2::password_hash::rand_core::{OsRng, RngCore};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::prelude::FromRow;

use crate::db::models::User;
use crate::db::{db_execute, db_fetch_all, db_fetch_one, db_fetch_optional, DbPool};

#[derive(Serialize, Deserialize, Debug, Clone, FromRow)]
pub struct Invite {
    pub id: i64,
    pub token_hash: String,
    pub email: Option<String>,
    pub role: String,
    pub created_by: String,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub fn hash_token(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn generate_raw_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub async fn create_invite(
    pool: &DbPool,
    email: Option<&str>,
    role: &str,
    created_by: &str,
    days_valid: i64,
) -> Result<(Invite, String), sqlx::Error> {
    let raw = generate_raw_token();
    let token_hash = hash_token(&raw);
    let days = days_valid.clamp(1, 30);
    let expires_at = Utc::now() + chrono::Duration::days(days);
    let invite = db_fetch_one!(
        pool,
        Invite,
        "INSERT INTO invites (token_hash, email, role, created_by, expires_at)
         VALUES (?, ?, ?, ?, ?)
         RETURNING *",
        &token_hash,
        email,
        role,
        created_by,
        expires_at
    )?;
    Ok((invite, raw))
}

pub async fn list_pending(pool: &DbPool) -> Result<Vec<Invite>, sqlx::Error> {
    db_fetch_all!(
        pool,
        Invite,
        "SELECT * FROM invites
         WHERE accepted_at IS NULL AND expires_at > NOW()
         ORDER BY created_at DESC"
    )
}

pub async fn get_pending_by_token(
    pool: &DbPool,
    raw_token: &str,
) -> Result<Option<Invite>, sqlx::Error> {
    let token_hash = hash_token(raw_token);
    db_fetch_optional!(
        pool,
        Invite,
        "SELECT * FROM invites
         WHERE token_hash = ? AND accepted_at IS NULL AND expires_at > NOW()",
        &token_hash
    )
}

pub async fn get_pending_by_email(
    pool: &DbPool,
    email: &str,
) -> Result<Option<Invite>, sqlx::Error> {
    db_fetch_optional!(
        pool,
        Invite,
        "SELECT * FROM invites
         WHERE lower(email) = lower(?) AND accepted_at IS NULL AND expires_at > NOW()
         ORDER BY created_at DESC
         LIMIT 1",
        email
    )
}

pub async fn revoke(pool: &DbPool, id: i64) -> Result<bool, sqlx::Error> {
    let n = db_execute!(
        pool,
        "DELETE FROM invites WHERE id = ? AND accepted_at IS NULL",
        id
    )?;
    Ok(n > 0)
}

pub async fn mark_accepted(pool: &DbPool, id: i64) -> Result<(), sqlx::Error> {
    db_execute!(
        pool,
        "UPDATE invites SET accepted_at = NOW() WHERE id = ?",
        id
    )?;
    Ok(())
}

/// Accept a local invite: atomically claim invite, then create user (single transaction).
pub async fn accept_local(
    pool: &DbPool,
    invite: &Invite,
    email: &str,
    password: &str,
) -> Result<User, sqlx::Error> {
    let mut tx = pool.as_pg().begin().await?;

    let claimed: Option<Invite> = sqlx::query_as(
        "UPDATE invites SET accepted_at = NOW()
         WHERE id = $1 AND accepted_at IS NULL AND expires_at > NOW()
         RETURNING *",
    )
    .bind(invite.id)
    .fetch_optional(&mut *tx)
    .await?;

    let claimed = claimed.ok_or_else(|| {
        sqlx::Error::Protocol("Invite not found, expired, or already accepted".into())
    })?;

    let password_hash = crate::db::users::hash_password(password)?;
    let user: User = sqlx::query_as(
        "INSERT INTO users (email, password_hash, role, auth_provider, is_bootstrap)
         VALUES ($1, $2, $3, 'local', false)
         RETURNING *",
    )
    .bind(email)
    .bind(&password_hash)
    .bind(&claimed.role)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(user)
}

/// Consume a pending OIDC invite and create the user in one transaction.
/// Returns `Ok(None)` when no invite exists (caller may allow open join).
pub async fn accept_oidc(pool: &DbPool, email: &str) -> Result<Option<User>, sqlx::Error> {
    let mut tx = pool.as_pg().begin().await?;

    let claimed: Option<Invite> = sqlx::query_as(
        "UPDATE invites SET accepted_at = NOW()
         WHERE id = (
           SELECT id FROM invites
           WHERE lower(email) = lower($1)
             AND accepted_at IS NULL
             AND expires_at > NOW()
           ORDER BY created_at DESC
           LIMIT 1
         )
         RETURNING *",
    )
    .bind(email)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(claimed) = claimed else {
        tx.rollback().await?;
        return Ok(None);
    };

    let existing: Option<User> = sqlx::query_as("SELECT * FROM users WHERE email = $1")
        .bind(email)
        .fetch_optional(&mut *tx)
        .await?;
    if let Some(u) = existing {
        tx.commit().await?;
        return Ok(Some(u));
    }

    let user: User = sqlx::query_as(
        "INSERT INTO users (email, password_hash, role, auth_provider, is_bootstrap)
         VALUES ($1, '', $2, 'oidc', false)
         RETURNING *",
    )
    .bind(email)
    .bind(&claimed.role)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Some(user))
}
