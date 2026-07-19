use crate::db::DbPool;

/// Create a session for the given email. Returns the session token.
/// Sessions expire after 7 days.
pub async fn create_session(pool: &DbPool, email: &str) -> Result<String, sqlx::Error> {
    use rand::Rng;
    let token: String = rand::rng()
        .sample_iter(&rand::distr::Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();

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
