use crate::db::models::*;
use crate::db::DbPool;

pub async fn list_repos(pool: &DbPool) -> Result<Vec<Repo>, sqlx::Error> {
    sqlx::query_as::<_, Repo>("SELECT * FROM repos ORDER BY full_name")
        .fetch_all(&pool.0)
        .await
}

pub async fn get_repo(pool: &DbPool, id: i64) -> Result<Option<Repo>, sqlx::Error> {
    sqlx::query_as::<_, Repo>("SELECT * FROM repos WHERE id = ?")
        .bind(id)
        .fetch_optional(&pool.0)
        .await
}

pub async fn get_repo_by_full_name(pool: &DbPool, full_name: &str) -> Result<Option<Repo>, sqlx::Error> {
    sqlx::query_as::<_, Repo>("SELECT * FROM repos WHERE full_name = ?")
        .bind(full_name)
        .fetch_optional(&pool.0)
        .await
}

pub async fn create_repo(pool: &DbPool, repo: &RepoCreate) -> Result<Repo, sqlx::Error> {
    sqlx::query_as::<_, Repo>(
        "INSERT INTO repos (github_id, full_name, owner, name, default_branch, installation_id, private)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING *",
    )
    .bind(repo.github_id)
    .bind(&repo.full_name)
    .bind(&repo.owner)
    .bind(&repo.name)
    .bind(&repo.default_branch)
    .bind(repo.installation_id)
    .bind(repo.private)
    .fetch_one(&pool.0)
    .await
}

pub async fn update_repo(
    pool: &DbPool,
    id: i64,
    config_json: &str,
    active: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE repos SET config_json = ?, active = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(config_json)
    .bind(active)
    .bind(id)
    .execute(&pool.0)
    .await?;
    Ok(())
}

pub async fn delete_repo(pool: &DbPool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM repos WHERE id = ?")
        .bind(id)
        .execute(&pool.0)
        .await?;
    Ok(())
}
