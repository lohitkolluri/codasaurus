pub const MIGRATIONS: &[&str] = &[
    // Migration 001: Initial schema
    "CREATE TABLE IF NOT EXISTS schema_version (
        version INTEGER PRIMARY KEY,
        applied_at TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE IF NOT EXISTS app_config (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE IF NOT EXISTS repos (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        github_id INTEGER UNIQUE,
        full_name TEXT NOT NULL,
        owner TEXT NOT NULL,
        name TEXT NOT NULL,
        default_branch TEXT,
        installation_id INTEGER NOT NULL,
        private INTEGER NOT NULL DEFAULT 0,
        active INTEGER NOT NULL DEFAULT 1,
        config_json TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE INDEX IF NOT EXISTS idx_repos_owner ON repos(owner);
    CREATE INDEX IF NOT EXISTS idx_repos_active ON repos(active);

    CREATE TABLE IF NOT EXISTS reviews (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        repo_id INTEGER NOT NULL REFERENCES repos(id),
        pr_number INTEGER NOT NULL,
        pr_title TEXT,
        pr_author TEXT,
        pr_base_branch TEXT,
        pr_head_branch TEXT,
        pr_head_sha TEXT,
        status TEXT NOT NULL DEFAULT 'pending'
            CHECK (status IN ('pending', 'running', 'passed', 'failed', 'error')),
        summary_json TEXT,
        started_at TEXT,
        completed_at TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE INDEX IF NOT EXISTS idx_reviews_repo ON reviews(repo_id);
    CREATE INDEX IF NOT EXISTS idx_reviews_status ON reviews(status);
    CREATE INDEX IF NOT EXISTS idx_reviews_created ON reviews(created_at DESC);

    CREATE TABLE IF NOT EXISTS findings (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        review_id INTEGER NOT NULL REFERENCES reviews(id),
        fingerprint TEXT UNIQUE,
        file_path TEXT NOT NULL,
        line_start INTEGER,
        line_end INTEGER,
        column_start INTEGER,
        column_end INTEGER,
        severity TEXT NOT NULL CHECK (severity IN ('blocking', 'warning', 'info')),
        detector TEXT NOT NULL,
        rule_id TEXT,
        message TEXT NOT NULL,
        suggested_fix TEXT,
        code_snippet TEXT,
        context TEXT,
        category TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE INDEX IF NOT EXISTS idx_findings_review ON findings(review_id);
    CREATE INDEX IF NOT EXISTS idx_findings_file ON findings(file_path);
    CREATE INDEX IF NOT EXISTS idx_findings_severity ON findings(severity);
    CREATE INDEX IF NOT EXISTS idx_findings_fingerprint ON findings(fingerprint);

    CREATE TABLE IF NOT EXISTS dismissals (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        fingerprint TEXT NOT NULL REFERENCES findings(fingerprint),
        dismissed_by TEXT,
        reason TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        UNIQUE(fingerprint)
    );

    CREATE TABLE IF NOT EXISTS audit_log (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        event_type TEXT NOT NULL,
        actor TEXT,
        target_type TEXT,
        target_id INTEGER,
        metadata_json TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE INDEX IF NOT EXISTS idx_audit_event ON audit_log(event_type);
    CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_log(created_at DESC);

    CREATE TABLE IF NOT EXISTS users (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        email TEXT UNIQUE NOT NULL,
        password_hash TEXT NOT NULL,
        role TEXT NOT NULL DEFAULT 'admin' CHECK (role IN ('admin', 'viewer')),
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );",
];

pub async fn run_migrations(pool: &sqlx::Pool<sqlx::Sqlite>) -> Result<(), sqlx::Error> {
    // Ensure schema_version table exists
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    for (i, migration) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as i64;

        // Check if this migration has already been applied
        let exists: bool = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM schema_version WHERE version = ?",
        )
        .bind(version)
        .fetch_one(pool)
        .await?
            > 0;

        if !exists {
            // Execute each statement individually
            for statement in split_sql(migration) {
                sqlx::query(&statement).execute(pool).await?;
            }

            // Record migration
            sqlx::query("INSERT INTO schema_version (version) VALUES (?)")
                .bind(version)
                .execute(pool)
                .await?;
        }
    }

    Ok(())
}

fn split_sql(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut string_char = ' ';
    let mut chars = sql.chars().peekable();

    while let Some(c) = chars.next() {
        if in_string {
            current.push(c);
            if c == string_char {
                // Handle escaped quotes
                if chars.peek() == Some(&string_char) {
                    chars.next();
                    current.push(string_char);
                } else {
                    in_string = false;
                }
            }
        } else if c == '\'' || c == '"' {
            in_string = true;
            string_char = c;
            current.push(c);
        } else if c == ';' {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                statements.push(trimmed);
            }
            current.clear();
        } else {
            current.push(c);
        }
    }

    // Handle trailing content
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        statements.push(trimmed);
    }

    statements
}
