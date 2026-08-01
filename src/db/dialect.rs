//! Adapt `?` placeholders to Postgres `$n` bind parameters.

/// Rewrite query text for Postgres (`$n` placeholders).
pub fn prepare(sql: &str) -> String {
    qmark_to_dollar(sql)
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
        let s = prepare("SELECT * FROM t WHERE a = ? AND b = ?");
        assert_eq!(s, "SELECT * FROM t WHERE a = $1 AND b = $2");
    }

    #[test]
    fn skips_qmark_in_strings() {
        let s = prepare("SELECT '?' FROM t WHERE a = ?");
        assert_eq!(s, "SELECT '?' FROM t WHERE a = $1");
    }
}
