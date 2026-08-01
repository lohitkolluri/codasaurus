//! Postgres smoke: migrations, queue claim, users/sessions.
//!
//! Uses `DATABASE_URL` or `CODASAURUS_TEST_DATABASE_URL`
//! (defaults to `postgres://codasaurus:codasaurus@127.0.0.1:5432/codasaurus`).

use codasaurus::bot::queue;
use codasaurus::db;

fn pg_url() -> String {
    std::env::var("DATABASE_URL")
        .or_else(|_| std::env::var("CODASAURUS_TEST_DATABASE_URL"))
        .unwrap_or_else(|_| "postgres://codasaurus:codasaurus@127.0.0.1:5432/codasaurus".into())
}

#[tokio::test]
async fn postgres_migrations_enqueue_and_claim() {
    let pool = db::create_pool(&pg_url())
        .await
        .expect("postgres create_pool / migrations");

    pool.ping().await.expect("ping");

    let repo = format!("acme/demo-{}", uuid::Uuid::new_v4().simple());
    let id = queue::enqueue(&pool, &repo, 42, "deadbeef", Some(1), "opened")
        .await
        .expect("enqueue");
    assert!(id > 0);

    let job = queue::claim_next(&pool, 600)
        .await
        .expect("claim")
        .expect("expected a job");
    assert_eq!(job.repo, repo);
    assert_eq!(job.pr_number, 42);

    queue::mark_done(&pool, job.id).await.expect("mark_done");

    let email = format!(
        "pg-smoke-{}@example.com",
        chrono::Utc::now().timestamp_millis()
    );
    let user = db::users::create_user(&pool, &email, "test-pass-123!", "admin")
        .await
        .expect("create_user");
    assert_eq!(user.email, email);
    let token = db::sessions::create_session(&pool, &user.email)
        .await
        .expect("session");
    assert_eq!(token.len(), 64);

    let _ = sqlx::query("DELETE FROM review_jobs WHERE repo = $1")
        .bind(&repo)
        .execute(pool.as_pg())
        .await;
}
