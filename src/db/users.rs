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
) -> Result<Option<User>, sqlx::Error> {
    let user: Option<User> =
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ?")
            .bind(email)
            .fetch_optional(&pool.0)
            .await?;

    match user {
        Some(u) => {
            let parsed_hash = PasswordHash::new(&u.password_hash)
                .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
            let argon2 = Argon2::default();
            match argon2.verify_password(password.as_bytes(), &parsed_hash) {
                Ok(_) => Ok(Some(u)),
                Err(_) => Ok(None),
            }
        }
        None => Ok(None),
    }
}

pub async fn get_user_by_email(pool: &DbPool, email: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(&pool.0)
        .await
}
