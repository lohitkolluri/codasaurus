use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use std::sync::LazyLock;

use crate::db::models::*;
use crate::db::{db_fetch_one, db_fetch_optional, db_scalar, DbPool};

static DUMMY_PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(b"codasaurus-timing-dummy-v1", &salt)
        .map(|h| h.to_string())
        .unwrap_or_else(|_| {
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

    db_fetch_one!(
        pool,
        User,
        "INSERT INTO users (email, password_hash, role, auth_provider) VALUES (?, ?, ?, 'local') RETURNING *",
        email,
        &password_hash,
        role
    )
}

pub async fn upsert_oidc_user(pool: &DbPool, email: &str, role: &str) -> Result<User, sqlx::Error> {
    if let Some(u) = db_fetch_optional!(pool, User, "SELECT * FROM users WHERE email = ?", email)? {
        return Ok(u);
    }
    db_fetch_one!(
        pool,
        User,
        "INSERT INTO users (email, password_hash, role, auth_provider) VALUES (?, '', ?, 'oidc') RETURNING *",
        email,
        role
    )
}

pub async fn verify_password(
    pool: &DbPool,
    email: &str,
    password: &str,
) -> Result<Option<UserView>, sqlx::Error> {
    let user: Option<User> =
        db_fetch_optional!(pool, User, "SELECT * FROM users WHERE email = ?", email)?;

    let (expected_hash, user_view) = match user {
        Some(u) if u.password_hash.is_empty() => {
            return Ok(None); // OIDC-only user
        }
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

pub async fn admin_exists(pool: &DbPool) -> Result<bool, sqlx::Error> {
    let count: i64 = db_scalar!(pool, i64, "SELECT COUNT(*) FROM users WHERE role = 'admin'")?;
    Ok(count > 0)
}
