//! Dual-backend query macros (`?` placeholders; dialect rewrites for Postgres).

/// Run `fetch_all` for a `FromRow` type against either backend.
macro_rules! db_fetch_all {
    ($pool:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        match __pool {
            $crate::db::DbPool::Sqlite(p) => {
                sqlx::query_as::<_, $ty>(&__sql)
                    $(.bind($bind))*
                    .fetch_all(p)
                    .await
            }
            $crate::db::DbPool::Postgres(p) => {
                sqlx::query_as::<_, $ty>(&__sql)
                    $(.bind($bind))*
                    .fetch_all(p)
                    .await
            }
        }
    }};
}

macro_rules! db_fetch_optional {
    ($pool:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        match __pool {
            $crate::db::DbPool::Sqlite(p) => {
                sqlx::query_as::<_, $ty>(&__sql)
                    $(.bind($bind))*
                    .fetch_optional(p)
                    .await
            }
            $crate::db::DbPool::Postgres(p) => {
                sqlx::query_as::<_, $ty>(&__sql)
                    $(.bind($bind))*
                    .fetch_optional(p)
                    .await
            }
        }
    }};
}

macro_rules! db_fetch_one {
    ($pool:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        match __pool {
            $crate::db::DbPool::Sqlite(p) => {
                sqlx::query_as::<_, $ty>(&__sql)
                    $(.bind($bind))*
                    .fetch_one(p)
                    .await
            }
            $crate::db::DbPool::Postgres(p) => {
                sqlx::query_as::<_, $ty>(&__sql)
                    $(.bind($bind))*
                    .fetch_one(p)
                    .await
            }
        }
    }};
}

macro_rules! db_execute {
    ($pool:expr, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        match __pool {
            $crate::db::DbPool::Sqlite(p) => {
                sqlx::query(&__sql)
                    $(.bind($bind))*
                    .execute(p)
                    .await
                    .map(|r| r.rows_affected())
            }
            $crate::db::DbPool::Postgres(p) => {
                sqlx::query(&__sql)
                    $(.bind($bind))*
                    .execute(p)
                    .await
                    .map(|r| r.rows_affected())
            }
        }
    }};
}

macro_rules! db_scalar {
    ($pool:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        match __pool {
            $crate::db::DbPool::Sqlite(p) => {
                sqlx::query_scalar::<_, $ty>(&__sql)
                    $(.bind($bind))*
                    .fetch_one(p)
                    .await
            }
            $crate::db::DbPool::Postgres(p) => {
                sqlx::query_scalar::<_, $ty>(&__sql)
                    $(.bind($bind))*
                    .fetch_one(p)
                    .await
            }
        }
    }};
}

macro_rules! db_scalar_optional {
    ($pool:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        match __pool {
            $crate::db::DbPool::Sqlite(p) => {
                sqlx::query_scalar::<_, $ty>(&__sql)
                    $(.bind($bind))*
                    .fetch_optional(p)
                    .await
            }
            $crate::db::DbPool::Postgres(p) => {
                sqlx::query_scalar::<_, $ty>(&__sql)
                    $(.bind($bind))*
                    .fetch_optional(p)
                    .await
            }
        }
    }};
}

pub(crate) use db_execute;
pub(crate) use db_fetch_all;
pub(crate) use db_fetch_one;
pub(crate) use db_fetch_optional;
pub(crate) use db_scalar;
pub(crate) use db_scalar_optional;
