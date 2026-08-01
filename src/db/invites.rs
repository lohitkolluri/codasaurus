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

pub async fn get_pending_by_token(pool: &DbPool, raw_token: &str) -> Result<Option<Invite>, sqlx::Error> {
    let token_hash = hash_token(raw_token);
    db_fetch_optional!(
        pool,
        Invite,
        "SELECT * FROM invites
         WHERE token_hash = ? AND accepted_at IS NULL AND expires_at > NOW()",
        &token_hash
    )
}

pub async fn get_pending_by_email(pool: &DbPool, email: &str) -> Result<Option<Invite>, sqlx::Error> {
    db_fetch_optional!(
        pool,
        Invite,
        "SELECT * FROM invites
         WHERE email = ? AND accepted_at IS NULL AND expires_at > NOW()
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

/// Accept a local invite: create user + mark invite accepted.
pub async fn accept_local(
    pool: &DbPool,
    invite: &Invite,
    email: &str,
    password: &str,
) -> Result<User, sqlx::Error> {
    let user = crate::db::users::create_user(pool, email, password, &invite.role).await?;
    mark_accepted(pool, invite.id).await?;
    Ok(user)
}

/// Consume a pending invite for an OIDC email (no password). Returns invite role if consumed.
pub async fn consume_for_oidc(pool: &DbPool, email: &str) -> Result<Option<String>, sqlx::Error> {
    if let Some(inv) = get_pending_by_email(pool, email).await? {
        mark_accepted(pool, inv.id).await?;
        return Ok(Some(inv.role));
    }
    Ok(None)
}
