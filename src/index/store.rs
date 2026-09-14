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
    // Edges first: the subquery reads the very rows the symbol DELETE removes,
    // so deleting symbols first would leave every stale edge behind forever.
    sqlx::query(
        "DELETE FROM repo_edges WHERE repo_full_name = $1 AND
         (from_symbol = $2 OR
          from_symbol IN (SELECT symbol_name FROM repo_symbols WHERE repo_full_name = $1 AND file_path = $2))",
    )
    .bind(repo_full_name)
    .bind(&file.file_path)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM repo_symbols WHERE repo_full_name = $1 AND file_path = $2")
        .bind(repo_full_name)
        .bind(&file.file_path)
        .execute(&mut *tx)
        .await?;
    insert_files(&mut tx, repo_full_name, std::slice::from_ref(file)).await?;
    upsert_status(&mut tx, repo_full_name, INDEX_READY, None).await?;
    tx.commit().await
}

type Tx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;

/// Bulk-insert via `UNNEST` arrays: one round-trip for the whole index instead
/// of one per symbol/edge (a full repo build is tens of thousands of rows).
async fn insert_files(tx: &mut Tx<'_>, repo: &str, files: &[FileIndex]) -> Result<(), sqlx::Error> {
    let sym_total: usize = files.iter().map(|f| f.symbols.len()).sum();
    let mut paths = Vec::with_capacity(sym_total);
    let mut names = Vec::with_capacity(sym_total);
    let mut kinds = Vec::with_capacity(sym_total);
    let mut signatures: Vec<Option<String>> = Vec::with_capacity(sym_total);
    let mut lines = Vec::with_capacity(sym_total);

    let edge_total: usize = files.iter().map(|f| f.edges.len()).sum();
    let mut from_symbols = Vec::with_capacity(edge_total);
    let mut to_symbols = Vec::with_capacity(edge_total);
    let mut edge_kinds = Vec::with_capacity(edge_total);

    for file in files {
        for sym in &file.symbols {
            paths.push(file.file_path.clone());
            names.push(sym.name.clone());
            kinds.push(sym.kind.clone());
            signatures.push(sym.signature.clone());
            lines.push(sym.line);
        }
        for edge in &file.edges {
            from_symbols.push(edge.from_symbol.clone());
            to_symbols.push(edge.to_symbol.clone());
            edge_kinds.push(edge.edge_kind.clone());
        }
    }

    if !names.is_empty() {
        sqlx::query(
            "INSERT INTO repo_symbols (repo_full_name, file_path, symbol_name, kind, signature, line)
             SELECT $1, * FROM UNNEST($2::text[], $3::text[], $4::text[], $5::text[], $6::bigint[])
             ON CONFLICT (repo_full_name, file_path, symbol_name, line) DO NOTHING",
        )
        .bind(repo)
        .bind(&paths)
        .bind(&names)
        .bind(&kinds)
        .bind(&signatures)
        .bind(&lines)
        .execute(&mut **tx)
        .await?;
    }

    if !from_symbols.is_empty() {
        sqlx::query(
            "INSERT INTO repo_edges (repo_full_name, from_symbol, to_symbol, edge_kind)
             SELECT $1, * FROM UNNEST($2::text[], $3::text[], $4::text[])
             ON CONFLICT (repo_full_name, from_symbol, to_symbol, edge_kind) DO NOTHING",
        )
        .bind(repo)
        .bind(&from_symbols)
        .bind(&to_symbols)
        .bind(&edge_kinds)
        .execute(&mut **tx)
        .await?;
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
