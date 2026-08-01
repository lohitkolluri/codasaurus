use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use std::sync::LazyLock;

use crate::db::models::*;
use crate::db::{db_execute, db_fetch_all, db_fetch_one, db_fetch_optional, db_scalar, DbPool};

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

fn hash_password(password: &str) -> Result<String, sqlx::Error> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))
}

pub async fn create_user(
    pool: &DbPool,
    email: &str,
    password: &str,
    role: &str,
) -> Result<User, sqlx::Error> {
    create_user_with_bootstrap(pool, email, password, role, false).await
}

/// Create the onboarding account (instance bootstrap / superuser).
pub async fn create_bootstrap_owner(
    pool: &DbPool,
    email: &str,
    password: &str,
) -> Result<User, sqlx::Error> {
    create_user_with_bootstrap(pool, email, password, "owner", true).await
}

async fn create_user_with_bootstrap(
    pool: &DbPool,
    email: &str,
    password: &str,
    role: &str,
    is_bootstrap: bool,
) -> Result<User, sqlx::Error> {
    let password_hash = hash_password(password)?;
    db_fetch_one!(
        pool,
        User,
        "INSERT INTO users (email, password_hash, role, auth_provider, is_bootstrap)
         VALUES (?, ?, ?, 'local', ?) RETURNING *",
        email,
        &password_hash,
        role,
        is_bootstrap
    )
}

pub async fn upsert_oidc_user(pool: &DbPool, email: &str, role: &str) -> Result<User, sqlx::Error> {
    if let Some(u) = db_fetch_optional!(pool, User, "SELECT * FROM users WHERE email = ?", email)? {
        return Ok(u);
    }
    db_fetch_one!(
        pool,
        User,
        "INSERT INTO users (email, password_hash, role, auth_provider, is_bootstrap)
         VALUES (?, '', ?, 'oidc', false) RETURNING *",
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
                is_bootstrap: u.is_bootstrap,
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

/// True if at least one owner (or legacy admin) exists.
pub async fn admin_exists(pool: &DbPool) -> Result<bool, sqlx::Error> {
    owner_exists(pool).await
}

pub async fn owner_exists(pool: &DbPool) -> Result<bool, sqlx::Error> {
    let count: i64 = db_scalar!(
        pool,
        i64,
        "SELECT COUNT(*) FROM users WHERE role IN ('owner', 'admin')"
    )?;
    Ok(count > 0)
}

pub async fn owner_count(pool: &DbPool) -> Result<i64, sqlx::Error> {
    db_scalar!(
        pool,
        i64,
        "SELECT COUNT(*) FROM users WHERE role IN ('owner', 'admin')"
    )
}

pub async fn get_user_by_email(pool: &DbPool, email: &str) -> Result<Option<User>, sqlx::Error> {
    db_fetch_optional!(pool, User, "SELECT * FROM users WHERE email = ?", email)
}

pub async fn get_user_by_id(pool: &DbPool, id: i64) -> Result<Option<User>, sqlx::Error> {
    db_fetch_optional!(pool, User, "SELECT * FROM users WHERE id = ?", id)
}

pub async fn list_users(pool: &DbPool) -> Result<Vec<User>, sqlx::Error> {
    db_fetch_all!(
        pool,
        User,
        "SELECT * FROM users ORDER BY is_bootstrap DESC, created_at ASC"
    )
}

pub async fn update_user_role(pool: &DbPool, id: i64, role: &str) -> Result<User, sqlx::Error> {
    db_fetch_one!(
        pool,
        User,
        "UPDATE users SET role = ? WHERE id = ? RETURNING *",
        role,
        id
    )
}

/// Move the bootstrap flag to another user (must already be an owner).
pub async fn transfer_bootstrap(pool: &DbPool, to_user_id: i64) -> Result<User, sqlx::Error> {
    db_execute!(
        pool,
        "UPDATE users SET is_bootstrap = FALSE WHERE is_bootstrap = TRUE"
    )?;
    db_fetch_one!(
        pool,
        User,
        "UPDATE users SET is_bootstrap = TRUE, role = 'owner' WHERE id = ? RETURNING *",
        to_user_id
    )
}

pub async fn delete_user(pool: &DbPool, id: i64) -> Result<bool, sqlx::Error> {
    let n = db_execute!(pool, "DELETE FROM users WHERE id = ?", id)?;
    Ok(n > 0)
}

pub async fn set_password(pool: &DbPool, email: &str, password: &str) -> Result<(), sqlx::Error> {
    let password_hash = hash_password(password)?;
    db_execute!(
        pool,
        "UPDATE users SET password_hash = ?, auth_provider = 'local' WHERE email = ?",
        &password_hash,
        email
    )?;
    Ok(())
}

pub async fn delete_sessions_for_email(pool: &DbPool, email: &str) -> Result<(), sqlx::Error> {
    db_execute!(pool, "DELETE FROM sessions WHERE email = ?", email)?;
    Ok(())
}
