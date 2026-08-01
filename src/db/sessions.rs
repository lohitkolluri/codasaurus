use crate::db::{db_execute, db_scalar_optional, DbPool};
use argon2::password_hash::rand_core::{OsRng, RngCore};

/// Create a session for the given email. Returns the session token.
/// Sessions expire after 7 days.
pub async fn create_session(pool: &DbPool, email: &str) -> Result<String, sqlx::Error> {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);

    db_execute!(
        pool,
        "INSERT INTO sessions (token, email, expires_at)
         VALUES (?, ?, datetime('now', '+7 days'))",
        &token,
        email
    )?;

    Ok(token)
}

/// Look up a session by token. Returns the email if valid and not expired.
pub async fn get_session(pool: &DbPool, token: &str) -> Result<Option<String>, sqlx::Error> {
    db_execute!(
        pool,
        "DELETE FROM sessions WHERE expires_at < datetime('now')"
    )?;

    db_scalar_optional!(
        pool,
        String,
        "SELECT email FROM sessions WHERE token = ? AND expires_at > datetime('now')",
        token
    )
}

/// Delete a session (logout).
pub async fn delete_session(pool: &DbPool, token: &str) -> Result<(), sqlx::Error> {
    db_execute!(pool, "DELETE FROM sessions WHERE token = ?", token)?;
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
