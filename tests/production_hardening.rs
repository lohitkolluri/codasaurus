//! Integration tests for auth, webhook security, config wiring, and retry helpers.
//!
//! Requires PostgreSQL. Set `DATABASE_URL` or `CODASAURUS_TEST_DATABASE_URL`
//! (defaults to `postgres://codasaurus:codasaurus@127.0.0.1:5432/codasaurus`).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use codasaurus::api::{self, AppState};
use codasaurus::db;
use codasaurus::retry::{github_request, RetryConfig};
use tower::ServiceExt;

async fn test_pool() -> db::DbPool {
    let url = std::env::var("DATABASE_URL")
        .or_else(|_| std::env::var("CODASAURUS_TEST_DATABASE_URL"))
        .unwrap_or_else(|_| "postgres://codasaurus:codasaurus@127.0.0.1:5432/codasaurus".into());
    db::create_pool(&url)
        .await
        .expect("create postgres test pool")
}

fn unique_email(prefix: &str) -> String {
    format!("{prefix}-{}@example.com", uuid::Uuid::new_v4().simple())
}

fn app(pool: db::DbPool) -> axum::Router {
    api::build_router(AppState { pool })
}

#[tokio::test]
async fn unauthenticated_settings_returns_401() {
    let pool = test_pool().await;
    let app = app(pool);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unauthenticated_reviews_returns_401() {
    let pool = test_pool().await;
    let app = app(pool);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/reviews")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unauthenticated_repos_returns_401() {
    let pool = test_pool().await;
    let app = app(pool);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/repos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn setup_status_is_public() {
    let pool = test_pool().await;
    let app = app(pool);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/setup/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn setup_admin_freeze_after_first_admin() {
    let pool = test_pool().await;
    let email = unique_email("owner");
    db::users::create_user(&pool, &email, "password123", "owner")
        .await
        .unwrap();

    let app = app(pool);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/setup/admin")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"email":"other@example.com","password":"password123"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn session_tokens_are_random() {
    let pool = test_pool().await;
    let email = unique_email("sess");
    db::users::create_user(&pool, &email, "password123", "owner")
        .await
        .unwrap();

    let t1 = db::sessions::create_session(&pool, &email).await.unwrap();
    let t2 = db::sessions::create_session(&pool, &email).await.unwrap();

    assert_ne!(t1, t2);
    assert_eq!(t1.len(), 64);
    assert!(t1.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn authenticated_settings_ok() {
    let pool = test_pool().await;
    let email = unique_email("settings");
    db::users::create_user(&pool, &email, "password123", "owner")
        .await
        .unwrap();
    let token = db::sessions::create_session(&pool, &email).await.unwrap();

    let app = app(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/settings")
                .header("cookie", format!("codasaurus_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn config_db_overlay_disables_secrets() {
    let pool = test_pool().await;
    db::config::set_config(&pool, "secrets_enabled", "false")
        .await
        .unwrap();

    let cfg = codasaurus::config::Config::load_for_bot(Some(&pool)).await;
    assert!(!cfg.checks.secrets);
    assert!(cfg.checks.hallucinated_imports); // default still on
}

#[tokio::test]
async fn github_jwt_rejects_bad_pem() {
    let err = codasaurus::github_jwt::create_app_jwt("123", "not-a-pem").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("private key") || msg.contains("PEM") || msg.contains("key"),
        "unexpected error: {msg}"
    );
}

#[tokio::test]
async fn ssrf_blocks_metadata() {
    assert!(codasaurus::ssrf::validate_llm_base_url("http://169.254.169.254/", false).is_err());
    assert!(codasaurus::ssrf::validate_llm_base_url("https://openrouter.ai/api/v1", false).is_ok());
}

#[tokio::test]
async fn webhook_delivery_dedup_persists() {
    let pool = test_pool().await;
    let id = format!("delivery-{}", uuid::Uuid::new_v4());

    let sql =
        "INSERT INTO webhook_deliveries (delivery_id) VALUES (?) ON CONFLICT(delivery_id) DO NOTHING";
    let prepared = pool.prepare_sql(sql);
    let first = sqlx::query(&prepared)
        .bind(&id)
        .execute(pool.as_pg())
        .await
        .unwrap()
        .rows_affected();
    assert_eq!(first, 1);

    let second = sqlx::query(&prepared)
        .bind(&id)
        .execute(pool.as_pg())
        .await
        .unwrap()
        .rows_affected();
    assert_eq!(second, 0);
}

#[tokio::test]
async fn review_state_sha_claim_is_exclusive() {
    let pool = test_pool().await;
    let state = codasaurus::state::ReviewState::from_pool(&pool);
    let repo = format!("owner/repo-{}", uuid::Uuid::new_v4().simple());

    assert!(state.try_claim_sha(&repo, 1, "abc123").await.unwrap());
    assert!(!state.try_claim_sha(&repo, 1, "abc123").await.unwrap());
    assert!(state.try_claim_sha(&repo, 1, "def456").await.unwrap());
}

#[tokio::test]
async fn github_request_fails_cleanly_on_unreachable() {
    let client = reqwest::Client::new();
    let result = github_request(&RetryConfig::quick(), "test", || {
        client.get("http://127.0.0.1:1/nope")
    })
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn viewer_cannot_write_settings() {
    let pool = test_pool().await;
    let email = unique_email("viewer");
    db::users::create_user(&pool, &email, "password123", "viewer")
        .await
        .unwrap();
    let token = db::sessions::create_session(&pool, &email).await.unwrap();
    let app = app(pool);

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/settings/llm_provider")
                .header("content-type", "application/json")
                .header("cookie", format!("codasaurus_session={token}"))
                .body(Body::from(r#"{"value":"disabled"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn maintainer_cannot_invite_but_can_list_repos() {
    let pool = test_pool().await;
    let email = unique_email("maint");
    db::users::create_user(&pool, &email, "password123", "maintainer")
        .await
        .unwrap();
    let token = db::sessions::create_session(&pool, &email).await.unwrap();
    let app = app(pool.clone());

    let invite = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/users/invites")
                .header("content-type", "application/json")
                .header("cookie", format!("codasaurus_session={token}"))
                .body(Body::from(r#"{"role":"viewer"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invite.status(), StatusCode::FORBIDDEN);

    let repos = app
        .oneshot(
            Request::builder()
                .uri("/api/repos")
                .header("cookie", format!("codasaurus_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(repos.status(), StatusCode::OK);
}

#[tokio::test]
async fn invite_accept_creates_session() {
    let pool = test_pool().await;
    let owner_email = unique_email("owner");
    db::users::create_user(&pool, &owner_email, "password123", "owner")
        .await
        .unwrap();
    let (_invite, raw) = db::invites::create_invite(
        &pool,
        Some(&unique_email("guest")),
        "viewer",
        &owner_email,
        7,
    )
    .await
    .unwrap();
    // Re-fetch invite email from pending
    let pending = db::invites::get_pending_by_token(&pool, &raw)
        .await
        .unwrap()
        .unwrap();
    let guest_email = pending.email.clone().unwrap();

    let app = app(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/auth/invite/{raw}/accept"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"password":"password123"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        set_cookie.contains("codasaurus_session="),
        "missing session cookie for {guest_email}: {set_cookie}"
    );
}

#[tokio::test]
async fn cannot_remove_last_owner() {
    let pool = test_pool().await;
    let email = unique_email("solo");
    let user = db::users::create_bootstrap_owner(&pool, &email, "password123")
        .await
        .unwrap();
    let token = db::sessions::create_session(&pool, &email).await.unwrap();
    let app = app(pool);

    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/users/{}", user.id))
                .header("cookie", format!("codasaurus_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admin_role_migrates_to_owner_rank() {
    assert_eq!(codasaurus::api::rbac::role_rank("admin"), 3);
    assert_eq!(codasaurus::api::rbac::normalize_role("admin"), "owner");
}
