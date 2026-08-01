//! SQLite → Postgres SQL adaptation for runtime queries.

use super::Backend;

/// Adapt a SQLite-flavoured statement for the active backend.
pub fn prepare(sql: &str, backend: Backend) -> String {
    match backend {
        Backend::Sqlite => sql.to_string(),
        Backend::Postgres => to_pg(sql),
    }
}

fn to_pg(sql: &str) -> String {
    let mut s = sql.to_string();

    // Literal datetime offsets (must run before generic datetime('now')).
    s = s.replace(
        "datetime('now', '-14 days')",
        "(CURRENT_TIMESTAMP - INTERVAL '14 days')",
    );
    s = s.replace(
        "datetime('now', '+7 days')",
        "(CURRENT_TIMESTAMP + INTERVAL '7 days')",
    );
    s = s.replace(
        "datetime('now', ?)",
        "(CURRENT_TIMESTAMP + CAST(? AS INTERVAL))",
    );
    s = s.replace("datetime('now')", "CURRENT_TIMESTAMP");

    // Prefer explicit ON CONFLICT in call sites; strip INSERT OR IGNORE prefix.
    s = s.replace("INSERT OR IGNORE INTO", "INSERT INTO");

    qmark_to_dollar(&s)
}

/// Convert `?` placeholders to `$1`, `$2`, … (skip `?` inside quotes).
fn qmark_to_dollar(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 8);
    let mut n = 0u32;
    let mut chars = sql.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(c) = chars.next() {
        if c == '\'' && !in_double {
            if in_single && chars.peek() == Some(&'\'') {
                out.push('\'');
                out.push('\'');
                chars.next();
                continue;
            }
            in_single = !in_single;
            out.push(c);
            continue;
        }
        if c == '"' && !in_single {
            in_double = !in_double;
            out.push(c);
            continue;
        }
        if c == '?' && !in_single && !in_double {
            n += 1;
            out.push('$');
            out.push_str(&n.to_string());
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_placeholders() {
        let s = to_pg("SELECT * FROM t WHERE a = ? AND b = ?");
        assert_eq!(s, "SELECT * FROM t WHERE a = $1 AND b = $2");
    }

    #[test]
    fn skips_qmark_in_strings() {
        let s = to_pg("SELECT '?' FROM t WHERE a = ?");
        assert_eq!(s, "SELECT '?' FROM t WHERE a = $1");
    }

    #[test]
    fn rewrites_datetime() {
        let s = to_pg("UPDATE t SET ts = datetime('now') WHERE id = ?");
        assert!(s.contains("CURRENT_TIMESTAMP"));
        assert!(s.contains("$1"));
    }
}
