use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

use crate::db::models::*;
use crate::db::DbPool;

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
    let user: Option<User> =
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ?")
            .bind(email)
            .fetch_optional(&pool.0)
            .await?;

    // Always run argon2 (even for missing users) to prevent timing attacks
    // that could reveal whether an email is registered.
    let (expected_hash, user_view) = match user {
        Some(u) => (u.password_hash.clone(), Some(UserView { email: u.email, role: u.role })),
        None => {
            // Use a dummy hash for unknown emails so verification takes ~same time
            let salt = argon2::password_hash::SaltString::generate(&mut OsRng);
            let dummy = Argon2::default()
                .hash_password(b"dummy-bc84a2e7f9", &salt)
                .map(|h| h.to_string())
                .unwrap_or_else(|_| "$argon2id$v=19$m=19456,t=2,p=1$dummy".into());
            (dummy, None)
        }
    };

    let parsed_hash = PasswordHash::new(&expected_hash)
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed_hash) {
        Ok(_) => Ok(user_view),
        Err(_) => Ok(None),
    }
}
