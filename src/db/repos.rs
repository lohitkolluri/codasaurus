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
           updated_at = CURRENT_TIMESTAMP
         RETURNING *",
        repo.github_id,
        &repo.full_name,
        &repo.owner,
        &repo.name,
        &repo.default_branch,
        repo.installation_id,
        repo.private,
        true
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
    db_execute!(pool, "DELETE FROM repos WHERE id = ?", id)?;
    Ok(())
}
