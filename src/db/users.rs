use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use std::sync::LazyLock;

use crate::db::models::*;
use crate::db::DbPool;

/// Precomputed Argon2 hash used for unknown emails so verify timing matches
/// known-email path (no fresh salt/hash on the miss path).
static DUMMY_PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(b"codasaurus-timing-dummy-v1", &salt)
        .map(|h| h.to_string())
        .unwrap_or_else(|_| {
            // Fallback constant — still a valid PHC string so verify runs.
            "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                .into()
        })
});

pub async fn create_user(
    pool: &DbPool,
    email: &str,
    password: &str,
    role: &str,
) -> Result<User, sqlx::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?
        .to_string();

    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (email, password_hash, role) VALUES (?, ?, ?) RETURNING *",
    )
    .bind(email)
    .bind(&password_hash)
    .bind(role)
    .fetch_one(&pool.0)
    .await?;

    Ok(user)
}

pub async fn verify_password(
    pool: &DbPool,
    email: &str,
    password: &str,
) -> Result<Option<UserView>, sqlx::Error> {
    let user: Option<User> = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(&pool.0)
        .await?;

    // Always verify against a real Argon2 hash so timing does not leak whether
    // the email exists. Unknown emails use a fixed precomputed dummy hash.
    let (expected_hash, user_view) = match user {
        Some(u) => (
            u.password_hash.clone(),
            Some(UserView {
                email: u.email,
                role: u.role,
            }),
        ),
        None => (DUMMY_PASSWORD_HASH.clone(), None),
    };

    let parsed_hash =
        PasswordHash::new(&expected_hash).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed_hash) {
        Ok(_) => Ok(user_view),
        Err(_) => Ok(None),
    }
}

/// Returns true if at least one admin user exists.
pub async fn admin_exists(pool: &DbPool) -> Result<bool, sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(&pool.0)
        .await?;
    Ok(count > 0)
}
