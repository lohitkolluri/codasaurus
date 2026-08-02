//! PostgreSQL persistence for the whole-repo symbol index.

use crate::db::DbPool;
use crate::index::extract::FileIndex;
use sqlx::Row;

const INDEX_READY: &str = "ready";
const INDEX_FAILED: &str = "failed";

/// Wipe the repo's index and insert a fresh full build in one transaction.
pub async fn replace_repo_index(
    pool: &DbPool,
    repo_full_name: &str,
    files: &[FileIndex],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.as_pg().begin().await?;
    sqlx::query("DELETE FROM repo_symbols WHERE repo_full_name = $1")
        .bind(repo_full_name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM repo_edges WHERE repo_full_name = $1")
        .bind(repo_full_name)
        .execute(&mut *tx)
        .await?;
    insert_files(&mut tx, repo_full_name, files).await?;
    upsert_status(&mut tx, repo_full_name, INDEX_READY, None).await?;
    tx.commit().await
}

/// Incremental update: re-parse one changed file, replacing only its rows.
pub async fn replace_file_index(
    pool: &DbPool,
    repo_full_name: &str,
    file: &FileIndex,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.as_pg().begin().await?;
    sqlx::query("DELETE FROM repo_symbols WHERE repo_full_name = $1 AND file_path = $2")
        .bind(repo_full_name)
        .bind(&file.file_path)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "DELETE FROM repo_edges WHERE repo_full_name = $1 AND from_symbol IN
         (SELECT symbol_name FROM repo_symbols WHERE repo_full_name = $1 AND file_path = $2)",
    )
    .bind(repo_full_name)
    .bind(&file.file_path)
    .execute(&mut *tx)
    .await?;
    insert_files(&mut tx, repo_full_name, std::slice::from_ref(file)).await?;
    upsert_status(&mut tx, repo_full_name, INDEX_READY, None).await?;
    tx.commit().await
}

type Tx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;

async fn insert_files(tx: &mut Tx<'_>, repo: &str, files: &[FileIndex]) -> Result<(), sqlx::Error> {
    for file in files {
        for sym in &file.symbols {
            sqlx::query(
                "INSERT INTO repo_symbols (repo_full_name, file_path, symbol_name, kind, signature, line)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (repo_full_name, file_path, symbol_name, line) DO NOTHING",
            )
            .bind(repo)
            .bind(&file.file_path)
            .bind(&sym.name)
            .bind(&sym.kind)
            .bind(&sym.signature)
            .bind(sym.line)
            .execute(&mut **tx)
            .await?;
        }
        for edge in &file.edges {
            sqlx::query(
                "INSERT INTO repo_edges (repo_full_name, from_symbol, to_symbol, edge_kind)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (repo_full_name, from_symbol, to_symbol, edge_kind) DO NOTHING",
            )
            .bind(repo)
            .bind(&edge.from_symbol)
            .bind(&edge.to_symbol)
            .bind(&edge.edge_kind)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

async fn upsert_status(
    tx: &mut Tx<'_>,
    repo: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO index_status (repo_full_name, status, built_at, error)
         VALUES ($1, $2, NOW(), $3)
         ON CONFLICT (repo_full_name)
         DO UPDATE SET status = EXCLUDED.status,
                       built_at = EXCLUDED.built_at,
                       error = EXCLUDED.error",
    )
    .bind(repo)
    .bind(status)
    .bind(error)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn mark_index_failed(pool: &DbPool, repo: &str, error: &str) {
    if let Ok(mut tx) = pool.as_pg().begin().await {
        let _ = upsert_status(&mut tx, repo, INDEX_FAILED, Some(error)).await;
        let _ = tx.commit().await;
    }
}

pub async fn index_status(pool: &DbPool, repo: &str) -> Option<(String, String)> {
    sqlx::query("SELECT status, COALESCE(error, '') FROM index_status WHERE repo_full_name = $1")
        .bind(repo)
        .fetch_optional(pool.as_pg())
        .await
        .ok()
        .flatten()
        .map(|row| (row.get(0), row.get(1)))
}

/// Callers of `symbol`: reverse CALLS/EXTENDS edges pointing at it.
pub async fn callers_of(
    pool: &DbPool,
    repo: &str,
    symbol: &str,
) -> Result<Vec<(String, String, String)>, sqlx::Error> {
    sqlx::query(
        "SELECT from_symbol, to_symbol, edge_kind FROM repo_edges
         WHERE repo_full_name = $1 AND to_symbol = $2
         ORDER BY from_symbol LIMIT 50",
    )
    .bind(repo)
    .bind(symbol)
    .fetch_all(pool.as_pg())
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect()
    })
}

/// Symbols defined in a file, so callers can be resolved by name.
pub async fn symbols_in_file(
    pool: &DbPool,
    repo: &str,
    file_path: &str,
) -> Result<Vec<(String, String, i64)>, sqlx::Error> {
    sqlx::query(
        "SELECT symbol_name, kind, COALESCE(line, 0) FROM repo_symbols
         WHERE repo_full_name = $1 AND file_path = $2
         ORDER BY line",
    )
    .bind(repo)
    .bind(file_path)
    .fetch_all(pool.as_pg())
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect()
    })
}

/// Files defining a symbol (reverse DEFINES edges).
pub async fn files_defining_symbol(
    pool: &DbPool,
    repo: &str,
    symbol: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query(
        "SELECT file_path FROM repo_symbols
         WHERE repo_full_name = $1 AND symbol_name = $2",
    )
    .bind(repo)
    .bind(symbol)
    .fetch_all(pool.as_pg())
    .await
    .map(|rows| rows.into_iter().map(|r| r.get(0)).collect())
}
