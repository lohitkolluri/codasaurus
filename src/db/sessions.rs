use crate::db::DbPool;
use argon2::password_hash::rand_core::{OsRng, RngCore};

/// Create a session for the given email. Returns the session token.
/// Sessions expire after 7 days.
pub async fn create_session(pool: &DbPool, email: &str) -> Result<String, sqlx::Error> {
    // Cryptographically random 32-byte token, stored as hex (64 chars).
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);

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

#[cfg(test)]
mod tests {
    #[test]
    fn session_tokens_are_hex_of_32_bytes() {
        let mut bytes = [0u8; 32];
        argon2::password_hash::rand_core::RngCore::fill_bytes(
            &mut argon2::password_hash::rand_core::OsRng,
            &mut bytes,
        );
        let token = hex::encode(bytes);
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
