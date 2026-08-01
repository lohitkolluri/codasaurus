use crate::db::{db_execute, db_scalar_optional, DbPool};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    hex::encode(digest)
}

/// Create a session for the given email. Returns the raw session token (cookie value).
/// Only the SHA-256 hash is stored in the database. Sessions expire after 7 days.
pub async fn create_session(pool: &DbPool, email: &str) -> Result<String, sqlx::Error> {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    let token_hash = hash_token(&token);

    db_execute!(
        pool,
        "INSERT INTO sessions (token, email, expires_at)
         VALUES (?, ?, NOW() + INTERVAL '7 days')",
        &token_hash,
        email
    )?;

    Ok(token)
}

/// Look up a session by raw token. Returns the email if valid and not expired.
/// Expired rows are pruned by periodic maintenance — not on the hot auth path.
pub async fn get_session(pool: &DbPool, token: &str) -> Result<Option<String>, sqlx::Error> {
    let token_hash = hash_token(token);
    db_scalar_optional!(
        pool,
        String,
        "SELECT email FROM sessions WHERE token = ? AND expires_at > NOW()",
        &token_hash
    )
}

/// Delete a session (logout) by raw token.
pub async fn delete_session(pool: &DbPool, token: &str) -> Result<(), sqlx::Error> {
    let token_hash = hash_token(token);
    db_execute!(pool, "DELETE FROM sessions WHERE token = ?", &token_hash)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::hash_token;

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
        assert_eq!(hash_token(&token).len(), 64);
        assert_ne!(hash_token(&token), token);
    }
}
