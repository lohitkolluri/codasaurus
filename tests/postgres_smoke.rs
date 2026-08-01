//! Optional Postgres smoke test.
//!
//! Set `CODASAURUS_TEST_DATABASE_URL=postgres://…` to run. Skipped otherwise.

use codasaurus::db::{self, DbPool};
use codasaurus::bot::queue;

fn pg_url() -> Option<String> {
    std::env::var("CODASAURUS_TEST_DATABASE_URL")
        .ok()
        .filter(|u| u.starts_with("postgres://") || u.starts_with("postgresql://"))
}

#[tokio::test]
async fn postgres_migrations_enqueue_and_claim() {
    let Some(url) = pg_url() else {
        eprintln!("skip: CODASAURUS_TEST_DATABASE_URL not set");
        return;
    };

    let pool = db::create_pool(&url)
        .await
        .expect("postgres create_pool / migrations");
    assert!(pool.is_postgres());

    pool.ping().await.expect("ping");

    let id = queue::enqueue(
        &pool,
        "acme/demo",
        42,
        "deadbeef",
        Some(1),
        "opened",
    )
    .await
    .expect("enqueue");
    assert!(id > 0);

    let job = queue::claim_next(&pool, 600)
        .await
        .expect("claim")
        .expect("expected a job");
    assert_eq!(job.repo, "acme/demo");
    assert_eq!(job.pr_number, 42);

    queue::mark_done(&pool, job.id).await.expect("mark_done");

    // Session / user path
    let email = format!("pg-smoke-{}@example.com", chrono::Utc::now().timestamp_millis());
    let user = db::users::create_user(&pool, &email, "test-pass-123!", "admin")
        .await
        .expect("create_user");
    assert_eq!(user.email, email);
    let token = db::sessions::create_session(&pool, &user.email)
        .await
        .expect("session");
    assert_eq!(token.len(), 64);

    // Cleanup smoke rows (best-effort)
    let _ = match &pool {
        DbPool::Postgres(p) => {
            sqlx::query("DELETE FROM review_jobs WHERE repo = $1")
                .bind("acme/demo")
                .execute(p)
                .await
        }
        DbPool::Sqlite(_) => unreachable!(),
    };
}
