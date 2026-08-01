//! Integration tests for auth, webhook security, config wiring, and retry helpers.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use codasaurus::api::{self, AppState};
use codasaurus::db;
use codasaurus::retry::{github_request, RetryConfig};
use tower::ServiceExt;

async fn test_pool() -> db::DbPool {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    // Keep dir alive for the process lifetime of this test
    std::mem::forget(dir);
    let url = format!("sqlite://{}?mode=rwc", path.display());
    db::create_pool(&url).await.expect("create test pool")
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
    db::users::create_user(&pool, "admin@example.com", "password123", "admin")
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
    db::users::create_user(&pool, "a@example.com", "password123", "admin")
        .await
        .unwrap();

    let t1 = db::sessions::create_session(&pool, "a@example.com")
        .await
        .unwrap();
    let t2 = db::sessions::create_session(&pool, "a@example.com")
        .await
        .unwrap();

    assert_ne!(t1, t2);
    assert_eq!(t1.len(), 64);
    assert!(t1.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn authenticated_settings_ok() {
    let pool = test_pool().await;
    db::users::create_user(&pool, "a@example.com", "password123", "admin")
        .await
        .unwrap();
    let token = db::sessions::create_session(&pool, "a@example.com")
        .await
        .unwrap();

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
    let id = "delivery-abc-123";

    // First insert via public API path (ON CONFLICT DO NOTHING).
    let sql = "INSERT INTO webhook_deliveries (delivery_id) VALUES (?) ON CONFLICT(delivery_id) DO NOTHING";
    let first = match &pool {
        codasaurus::db::DbPool::Sqlite(p) => sqlx::query(sql)
            .bind(id)
            .execute(p)
            .await
            .unwrap()
            .rows_affected(),
        codasaurus::db::DbPool::Postgres(p) => {
            let s = pool.prepare_sql(sql);
            sqlx::query(&s)
                .bind(id)
                .execute(p)
                .await
                .unwrap()
                .rows_affected()
        }
    };
    assert_eq!(first, 1);

    let second = match &pool {
        codasaurus::db::DbPool::Sqlite(p) => sqlx::query(sql)
            .bind(id)
            .execute(p)
            .await
            .unwrap()
            .rows_affected(),
        codasaurus::db::DbPool::Postgres(p) => {
            let s = pool.prepare_sql(sql);
            sqlx::query(&s)
                .bind(id)
                .execute(p)
                .await
                .unwrap()
                .rows_affected()
        }
    };
    assert_eq!(second, 0);
}

#[tokio::test]
async fn review_state_sha_claim_is_exclusive() {
    let pool = test_pool().await;
    let state = codasaurus::state::ReviewState::from_pool(&pool);

    assert!(state
        .try_claim_sha("owner/repo", 1, "abc123")
        .await
        .unwrap());
    // Same SHA again should fail claim
    assert!(!state
        .try_claim_sha("owner/repo", 1, "abc123")
        .await
        .unwrap());
    // New SHA should succeed
    assert!(state
        .try_claim_sha("owner/repo", 1, "def456")
        .await
        .unwrap());
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
