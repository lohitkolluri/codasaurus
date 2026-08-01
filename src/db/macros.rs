//! Postgres query macros (`?` placeholders rewritten via `dialect::prepare`).

/// Run `fetch_all` for a `FromRow` type.
macro_rules! db_fetch_all {
    ($pool:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        sqlx::query_as::<_, $ty>(&__sql)
            $(.bind($bind))*
            .fetch_all(__pool.as_pg())
            .await
    }};
}

macro_rules! db_fetch_optional {
    ($pool:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        sqlx::query_as::<_, $ty>(&__sql)
            $(.bind($bind))*
            .fetch_optional(__pool.as_pg())
            .await
    }};
}

macro_rules! db_fetch_one {
    ($pool:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        sqlx::query_as::<_, $ty>(&__sql)
            $(.bind($bind))*
            .fetch_one(__pool.as_pg())
            .await
    }};
}

macro_rules! db_execute {
    ($pool:expr, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        sqlx::query(&__sql)
            $(.bind($bind))*
            .execute(__pool.as_pg())
            .await
            .map(|r| r.rows_affected())
    }};
}

macro_rules! db_scalar {
    ($pool:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        sqlx::query_scalar::<_, $ty>(&__sql)
            $(.bind($bind))*
            .fetch_one(__pool.as_pg())
            .await
    }};
}

macro_rules! db_scalar_optional {
    ($pool:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let __pool = $pool;
        let __sql = __pool.prepare_sql($sql);
        sqlx::query_scalar::<_, $ty>(&__sql)
            $(.bind($bind))*
            .fetch_optional(__pool.as_pg())
            .await
    }};
}

pub(crate) use db_execute;
pub(crate) use db_fetch_all;
pub(crate) use db_fetch_one;
pub(crate) use db_fetch_optional;
pub(crate) use db_scalar;
pub(crate) use db_scalar_optional;
