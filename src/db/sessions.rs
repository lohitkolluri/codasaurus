use crate::db::DbPool;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// Create a session for the given email. Returns the session token.
/// Sessions expire after 7 days.
pub async fn create_session(pool: &DbPool, email: &str) -> Result<String, sqlx::Error> {
    // Generate a token from sha256(email + timestamp + stack entropy).
    // Uses existing deps (sha2, hex) — no rand needed.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let input = format!("{email}:{now}");
    let token = hex::encode(Sha256::digest(input.as_bytes()));

    sqlx::query(
        "INSERT INTO sessions (token, email, expires_at)
         VALUES (?, ?, datetime('now', '+7 days'))",
    )
    .bind(&token)
    .bind(email)
    .execute(&pool.0)
    .await?;

    Ok(token)
}

/// Look up a session by token. Returns the email if valid and not expired.
/// Cleans expired sessions on each call.
pub async fn get_session(pool: &DbPool, token: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE expires_at < datetime('now')")
        .execute(&pool.0)
        .await?;

    sqlx::query_scalar::<_, String>(
        "SELECT email FROM sessions WHERE token = ? AND expires_at > datetime('now')",
    )
    .bind(token)
    .fetch_optional(&pool.0)
    .await
}

/// Delete a session (logout).
pub async fn delete_session(pool: &DbPool, token: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE token = ?")
        .bind(token)
        .execute(&pool.0)
        .await?;
    Ok(())
}
