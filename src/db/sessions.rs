use crate::db::DbPool;

/// Create a session for the given email. Returns the session token.
/// Sessions expire after 7 days.
pub async fn create_session(pool: &DbPool, email: &str) -> Result<String, sqlx::Error> {
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Generate a token from a hash of email + timestamp + random offset
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let input = format!("{}:{}:{}", email, now, rand_offset());
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

/// Simple random offset using the address of a stack variable (no rand dependency).
fn rand_offset() -> u64 {
    let x: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    // Mix in the address of a stack variable for some entropy
    let stack_ptr = &x as *const u64 as u64;
    x ^ (stack_ptr >> 3)
}

/// Look up a session by token. Returns the email if the session is valid
/// (exists and not expired). Deletes expired sessions silently.
pub async fn get_session(pool: &DbPool, token: &str) -> Result<Option<String>, sqlx::Error> {
    // Clean expired sessions first
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
