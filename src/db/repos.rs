use crate::db::models::*;
use crate::db::{db_execute, db_fetch_all, db_fetch_one, db_fetch_optional, DbPool};

pub async fn list_repos(pool: &DbPool) -> Result<Vec<Repo>, sqlx::Error> {
    db_fetch_all!(pool, Repo, "SELECT * FROM repos ORDER BY full_name")
}

pub async fn get_repo(pool: &DbPool, id: i64) -> Result<Option<Repo>, sqlx::Error> {
    db_fetch_optional!(pool, Repo, "SELECT * FROM repos WHERE id = ?", id)
}

pub async fn get_repo_by_full_name(
    pool: &DbPool,
    full_name: &str,
) -> Result<Option<Repo>, sqlx::Error> {
    db_fetch_optional!(
        pool,
        Repo,
        "SELECT * FROM repos WHERE full_name = ?",
        full_name
    )
}

pub async fn create_repo(pool: &DbPool, repo: &RepoCreate) -> Result<Repo, sqlx::Error> {
    create_repo_with_active(pool, repo, false).await
}

/// Register a repo from an installation webhook with reviews enabled by default.
pub async fn create_repo_from_installation(
    pool: &DbPool,
    repo: &RepoCreate,
) -> Result<Repo, sqlx::Error> {
    create_repo_with_active(pool, repo, true).await
}

async fn create_repo_with_active(
    pool: &DbPool,
    repo: &RepoCreate,
    active: bool,
) -> Result<Repo, sqlx::Error> {
    db_fetch_one!(
        pool,
        Repo,
        "INSERT INTO repos (github_id, full_name, owner, name, default_branch, installation_id, private, active)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(full_name) DO UPDATE SET
           github_id = excluded.github_id,
           installation_id = excluded.installation_id,
           private = excluded.private,
           default_branch = COALESCE(excluded.default_branch, repos.default_branch),
           -- Reactivate on reinstall; never deactivate an already-active repo via upsert.
           active = repos.active OR excluded.active,
           updated_at = CURRENT_TIMESTAMP
         RETURNING *",
        repo.github_id,
        &repo.full_name,
        &repo.owner,
        &repo.name,
        &repo.default_branch,
        repo.installation_id,
        repo.private,
        active
    )
}

pub async fn update_repo(
    pool: &DbPool,
    id: i64,
    config_json: &str,
    active: bool,
) -> Result<(), sqlx::Error> {
    db_execute!(
        pool,
        "UPDATE repos SET config_json = ?, active = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        config_json,
        active,
        id
    )?;
    Ok(())
}

pub async fn delete_repo(pool: &DbPool, id: i64) -> Result<(), sqlx::Error> {
    let mut tx = pool.as_pg().begin().await?;
    let full_name: Option<(String,)> = sqlx::query_as("SELECT full_name FROM repos WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    if let Some((ref name,)) = full_name {
        let like = format!("{name}/%");
        let _ = sqlx::query("DELETE FROM review_jobs WHERE repo = $1")
            .bind(name)
            .execute(&mut *tx)
            .await;
        let _ = sqlx::query("DELETE FROM review_comments WHERE repo_pr LIKE $1")
            .bind(&like)
            .execute(&mut *tx)
            .await;
        let _ = sqlx::query("DELETE FROM reviewed_commits WHERE repo_pr LIKE $1")
            .bind(&like)
            .execute(&mut *tx)
            .await;
        let _ = sqlx::query("DELETE FROM dismissed_findings WHERE repo_full_name = $1")
            .bind(name)
            .execute(&mut *tx)
            .await;
        let _ = sqlx::query("DELETE FROM learned_rules WHERE repo_full_name = $1")
            .bind(name)
            .execute(&mut *tx)
            .await;
    }
    // Prefer explicit cleanup so installs without ON DELETE CASCADE still succeed.
    let _ = sqlx::query(
        "DELETE FROM findings WHERE review_id IN (SELECT id FROM reviews WHERE repo_id = $1)",
    )
    .bind(id)
    .execute(&mut *tx)
    .await;
    let _ = sqlx::query("DELETE FROM reviews WHERE repo_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await;
    sqlx::query("DELETE FROM repos WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
