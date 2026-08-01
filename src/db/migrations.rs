use sqlx::{PgPool, SqlitePool};

/// Active SQLite schema.
const SQLITE_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS app_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS repos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    github_id BIGINT UNIQUE,
    full_name TEXT NOT NULL,
    owner TEXT NOT NULL,
    name TEXT NOT NULL,
    default_branch TEXT,
    installation_id BIGINT NOT NULL,
    private INTEGER NOT NULL DEFAULT 0,
    active INTEGER NOT NULL DEFAULT 0,
    config_json TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_repos_owner ON repos(owner);
CREATE INDEX IF NOT EXISTS idx_repos_active ON repos(active);
CREATE UNIQUE INDEX IF NOT EXISTS idx_repos_full_name ON repos(full_name);

CREATE TABLE IF NOT EXISTS reviews (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id INTEGER NOT NULL REFERENCES repos(id),
    pr_number BIGINT NOT NULL,
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
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
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
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_findings_review ON findings(review_id);
CREATE INDEX IF NOT EXISTS idx_findings_file ON findings(file_path);
CREATE INDEX IF NOT EXISTS idx_findings_severity ON findings(severity);
CREATE INDEX IF NOT EXISTS idx_findings_fingerprint ON findings(fingerprint);
CREATE INDEX IF NOT EXISTS idx_findings_detector ON findings(detector);

CREATE TABLE IF NOT EXISTS dismissals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    fingerprint TEXT NOT NULL REFERENCES findings(fingerprint),
    dismissed_by TEXT,
    reason TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(fingerprint)
);

CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,
    actor TEXT,
    target_type TEXT,
    target_id BIGINT,
    metadata_json TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_audit_event ON audit_log(event_type);
CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_log(created_at DESC);

CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'admin' CHECK (role IN ('admin', 'viewer')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS sessions (
    token TEXT PRIMARY KEY,
    email TEXT NOT NULL REFERENCES users(email) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);

CREATE TABLE IF NOT EXISTS webhook_deliveries (
    delivery_id TEXT PRIMARY KEY,
    received_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS review_comments (
    repo_pr TEXT PRIMARY KEY,
    comment_id INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS reviewed_commits (
    repo_pr TEXT PRIMARY KEY,
    head_sha TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'completed'
        CHECK (status IN ('in_progress', 'completed')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS dismissed_findings (
    fingerprint TEXT PRIMARY KEY,
    detector TEXT NOT NULL,
    file TEXT NOT NULL,
    line INTEGER NOT NULL,
    message TEXT NOT NULL,
    dismissed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS learned_rules (
    id TEXT PRIMARY KEY,
    detector TEXT NOT NULL,
    file_pattern TEXT,
    message_pattern TEXT,
    action TEXT NOT NULL DEFAULT 'ignore',
    reason TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_dismissed_fingerprint ON dismissed_findings(fingerprint);
CREATE INDEX IF NOT EXISTS idx_dismissed_detector ON dismissed_findings(detector);
CREATE INDEX IF NOT EXISTS idx_learned_detector ON learned_rules(detector);
";

const SQLITE_SCHEMA_V3: &str = "
CREATE TABLE IF NOT EXISTS webhook_deliveries (
    delivery_id TEXT PRIMARY KEY,
    received_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS review_comments (
    repo_pr TEXT PRIMARY KEY,
    comment_id INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS reviewed_commits (
    repo_pr TEXT PRIMARY KEY,
    head_sha TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'completed'
        CHECK (status IN ('in_progress', 'completed')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS dismissed_findings (
    fingerprint TEXT PRIMARY KEY,
    detector TEXT NOT NULL,
    file TEXT NOT NULL,
    line INTEGER NOT NULL,
    message TEXT NOT NULL,
    dismissed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS learned_rules (
    id TEXT PRIMARY KEY,
    detector TEXT NOT NULL,
    file_pattern TEXT,
    message_pattern TEXT,
    action TEXT NOT NULL DEFAULT 'ignore',
    reason TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_dismissed_fingerprint ON dismissed_findings(fingerprint);
CREATE INDEX IF NOT EXISTS idx_learned_detector ON learned_rules(detector);
";

pub async fn run_migrations_sqlite(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version BIGINT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;

    let current = current.unwrap_or(0);

    if current < 2 {
        for statement in split_sql(SQLITE_SCHEMA) {
            sqlx::query(&statement).execute(pool).await?;
        }
        for pragma in [
            "PRAGMA journal_mode = WAL",
            "PRAGMA synchronous = NORMAL",
            "PRAGMA cache_size = -8000",
            "PRAGMA busy_timeout = 5000",
            "PRAGMA foreign_keys = ON",
        ] {
            let _ = sqlx::query(pragma).execute(pool).await;
        }
        sqlx::query("INSERT OR IGNORE INTO schema_version (version) VALUES (2)")
            .execute(pool)
            .await?;
    }

    if current < 3 {
        for statement in split_sql(SQLITE_SCHEMA_V3) {
            sqlx::query(&statement).execute(pool).await?;
        }
        // Migrate reviewed_commits: add status column if upgrading from old side DB isn't needed;
        // main-DB table is created fresh above.
        sqlx::query("INSERT OR IGNORE INTO schema_version (version) VALUES (3)")
            .execute(pool)
            .await?;
    }

    if current < 4 {
        // Deduplicate before unique index (keep lowest id per full_name).
        sqlx::query(
            "DELETE FROM repos WHERE id NOT IN (SELECT MIN(id) FROM repos GROUP BY full_name)",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_repos_full_name ON repos(full_name)")
            .execute(pool)
            .await?;
        // Bound webhook dedup table growth
        let _ = sqlx::query(
            "DELETE FROM webhook_deliveries WHERE received_at < datetime('now', '-14 days')",
        )
        .execute(pool)
        .await;
        sqlx::query("INSERT OR IGNORE INTO schema_version (version) VALUES (4)")
            .execute(pool)
            .await?;
    }

    if current < 5 {
        for statement in [
            "CREATE INDEX IF NOT EXISTS idx_reviews_repo_created ON reviews(repo_id, created_at DESC)",
            "CREATE INDEX IF NOT EXISTS idx_reviews_repo_status ON reviews(repo_id, status)",
        ] {
            sqlx::query(statement).execute(pool).await?;
        }
        sqlx::query("INSERT OR IGNORE INTO schema_version (version) VALUES (5)")
            .execute(pool)
            .await?;
    }

    if current < 6 {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS review_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                repo TEXT NOT NULL,
                pr_number INTEGER NOT NULL,
                head_sha TEXT NOT NULL,
                installation_id INTEGER,
                action TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'running', 'done', 'failed')),
                attempts INTEGER NOT NULL DEFAULT 0,
                last_error TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_review_jobs_repo_pr_sha
             ON review_jobs(repo, pr_number, head_sha)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_review_jobs_status_created
             ON review_jobs(status, created_at)",
        )
        .execute(pool)
        .await?;
        sqlx::query("INSERT OR IGNORE INTO schema_version (version) VALUES (6)")
            .execute(pool)
            .await?;
    }

    if current < 7 {
        // Deduplicate reviews before unique (repo, pr, sha) index.
        sqlx::query(
            "DELETE FROM findings WHERE review_id NOT IN (
               SELECT MIN(id) FROM reviews GROUP BY repo_id, pr_number, COALESCE(pr_head_sha, '')
             )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "DELETE FROM reviews WHERE id NOT IN (
               SELECT MIN(id) FROM reviews GROUP BY repo_id, pr_number, COALESCE(pr_head_sha, '')
             )",
        )
        .execute(pool)
        .await?;
        // Normalize NULL sha → '' so unique index is effective.
        sqlx::query("UPDATE reviews SET pr_head_sha = '' WHERE pr_head_sha IS NULL")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_repo_pr_sha
             ON reviews(repo_id, pr_number, pr_head_sha)",
        )
        .execute(pool)
        .await?;
        // Multi-replica lease column on reviewed_commits (ignore if already present).
        let _ = sqlx::query(
            "ALTER TABLE reviewed_commits ADD COLUMN lease_owner TEXT NOT NULL DEFAULT ''",
        )
        .execute(pool)
        .await;
        sqlx::query("INSERT OR IGNORE INTO schema_version (version) VALUES (7)")
            .execute(pool)
            .await?;
    }

    if current < 8 {
        let _ =
            sqlx::query("ALTER TABLE users ADD COLUMN auth_provider TEXT NOT NULL DEFAULT 'local'")
                .execute(pool)
                .await;
        sqlx::query("INSERT OR IGNORE INTO schema_version (version) VALUES (8)")
            .execute(pool)
            .await?;
    }

    if current < 9 {
        for stmt in [
            "CREATE INDEX IF NOT EXISTS idx_findings_detector ON findings(detector)",
            "CREATE INDEX IF NOT EXISTS idx_dismissed_detector ON dismissed_findings(detector)",
            "CREATE INDEX IF NOT EXISTS idx_webhook_received_at ON webhook_deliveries(received_at)",
            "CREATE INDEX IF NOT EXISTS idx_review_jobs_status_updated ON review_jobs(status, updated_at)",
        ] {
            sqlx::query(stmt).execute(pool).await?;
        }
        sqlx::query("INSERT OR IGNORE INTO schema_version (version) VALUES (9)")
            .execute(pool)
            .await?;
    }

    if current < 10 {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS agent_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                agent TEXT NOT NULL,
                event_type TEXT NOT NULL,
                model TEXT,
                tokens_in INTEGER,
                tokens_out INTEGER,
                cost_usd_est REAL,
                latency_ms INTEGER,
                outcome TEXT,
                payload TEXT
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_agent_events_ts ON agent_events(ts)")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_agent_events_type_ts ON agent_events(event_type, ts)",
        )
        .execute(pool)
        .await?;
        sqlx::query("INSERT OR IGNORE INTO schema_version (version) VALUES (10)")
            .execute(pool)
            .await?;
    }

    Ok(())
}

const PG_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version BIGINT PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE TABLE IF NOT EXISTS app_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE TABLE IF NOT EXISTS repos (
    id BIGSERIAL PRIMARY KEY,
    github_id BIGINT UNIQUE,
    full_name TEXT NOT NULL,
    owner TEXT NOT NULL,
    name TEXT NOT NULL,
    default_branch TEXT,
    installation_id BIGINT NOT NULL,
    private BOOLEAN NOT NULL DEFAULT FALSE,
    active BOOLEAN NOT NULL DEFAULT FALSE,
    config_json TEXT,
    created_at TEXT NOT NULL DEFAULT (NOW()::text),
    updated_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE INDEX IF NOT EXISTS idx_repos_owner ON repos(owner);
CREATE INDEX IF NOT EXISTS idx_repos_active ON repos(active);
CREATE UNIQUE INDEX IF NOT EXISTS idx_repos_full_name ON repos(full_name);

CREATE TABLE IF NOT EXISTS reviews (
    id BIGSERIAL PRIMARY KEY,
    repo_id BIGINT NOT NULL REFERENCES repos(id),
    pr_number BIGINT NOT NULL,
    pr_title TEXT,
    pr_author TEXT,
    pr_base_branch TEXT,
    pr_head_branch TEXT,
    pr_head_sha TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'passed', 'failed', 'error')),
    summary_json TEXT,
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE INDEX IF NOT EXISTS idx_reviews_repo ON reviews(repo_id);
CREATE INDEX IF NOT EXISTS idx_reviews_status ON reviews(status);
CREATE INDEX IF NOT EXISTS idx_reviews_created ON reviews(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reviews_repo_created ON reviews(repo_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reviews_repo_status ON reviews(repo_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_repo_pr_sha
    ON reviews(repo_id, pr_number, pr_head_sha);

CREATE TABLE IF NOT EXISTS findings (
    id BIGSERIAL PRIMARY KEY,
    review_id BIGINT NOT NULL REFERENCES reviews(id),
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
    created_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE INDEX IF NOT EXISTS idx_findings_review ON findings(review_id);
CREATE INDEX IF NOT EXISTS idx_findings_file ON findings(file_path);
CREATE INDEX IF NOT EXISTS idx_findings_severity ON findings(severity);
CREATE INDEX IF NOT EXISTS idx_findings_fingerprint ON findings(fingerprint);
CREATE INDEX IF NOT EXISTS idx_findings_detector ON findings(detector);

CREATE TABLE IF NOT EXISTS dismissals (
    id BIGSERIAL PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    dismissed_by TEXT,
    reason TEXT,
    created_at TEXT NOT NULL DEFAULT (NOW()::text),
    UNIQUE(fingerprint)
);

CREATE TABLE IF NOT EXISTS audit_log (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL,
    actor TEXT,
    target_type TEXT,
    target_id BIGINT,
    metadata_json TEXT,
    created_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE INDEX IF NOT EXISTS idx_audit_event ON audit_log(event_type);
CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_log(created_at DESC);

CREATE TABLE IF NOT EXISTS users (
    id BIGSERIAL PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL DEFAULT '',
    role TEXT NOT NULL DEFAULT 'admin' CHECK (role IN ('admin', 'viewer')),
    auth_provider TEXT NOT NULL DEFAULT 'local',
    created_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE TABLE IF NOT EXISTS sessions (
    token TEXT PRIMARY KEY,
    email TEXT NOT NULL REFERENCES users(email) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (NOW()::text),
    expires_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);

CREATE TABLE IF NOT EXISTS webhook_deliveries (
    delivery_id TEXT PRIMARY KEY,
    received_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE INDEX IF NOT EXISTS idx_webhook_received_at ON webhook_deliveries(received_at);

CREATE TABLE IF NOT EXISTS review_comments (
    repo_pr TEXT PRIMARY KEY,
    comment_id BIGINT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE TABLE IF NOT EXISTS reviewed_commits (
    repo_pr TEXT PRIMARY KEY,
    head_sha TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'completed'
        CHECK (status IN ('in_progress', 'completed')),
    lease_owner TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE TABLE IF NOT EXISTS dismissed_findings (
    fingerprint TEXT PRIMARY KEY,
    detector TEXT NOT NULL,
    file TEXT NOT NULL,
    line INTEGER NOT NULL,
    message TEXT NOT NULL,
    dismissed_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE TABLE IF NOT EXISTS learned_rules (
    id TEXT PRIMARY KEY,
    detector TEXT NOT NULL,
    file_pattern TEXT,
    message_pattern TEXT,
    action TEXT NOT NULL DEFAULT 'ignore',
    reason TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE INDEX IF NOT EXISTS idx_dismissed_fingerprint ON dismissed_findings(fingerprint);
CREATE INDEX IF NOT EXISTS idx_dismissed_detector ON dismissed_findings(detector);
CREATE INDEX IF NOT EXISTS idx_learned_detector ON learned_rules(detector);

CREATE TABLE IF NOT EXISTS review_jobs (
    id BIGSERIAL PRIMARY KEY,
    repo TEXT NOT NULL,
    pr_number BIGINT NOT NULL,
    head_sha TEXT NOT NULL,
    installation_id BIGINT,
    action TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'done', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (NOW()::text),
    updated_at TEXT NOT NULL DEFAULT (NOW()::text)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_review_jobs_repo_pr_sha
    ON review_jobs(repo, pr_number, head_sha);
CREATE INDEX IF NOT EXISTS idx_review_jobs_status_created
    ON review_jobs(status, created_at);
CREATE INDEX IF NOT EXISTS idx_review_jobs_status_updated
    ON review_jobs(status, updated_at);

CREATE TABLE IF NOT EXISTS agent_events (
    id BIGSERIAL PRIMARY KEY,
    ts TEXT NOT NULL DEFAULT (NOW()::text),
    agent TEXT NOT NULL,
    event_type TEXT NOT NULL,
    model TEXT,
    tokens_in INTEGER,
    tokens_out INTEGER,
    cost_usd_est DOUBLE PRECISION,
    latency_ms INTEGER,
    outcome TEXT,
    payload TEXT
);

CREATE INDEX IF NOT EXISTS idx_agent_events_ts ON agent_events(ts);
CREATE INDEX IF NOT EXISTS idx_agent_events_type_ts ON agent_events(event_type, ts);
"#;

pub async fn run_migrations_postgres(pool: &PgPool) -> Result<(), sqlx::Error> {
    for statement in split_sql(PG_SCHEMA) {
        sqlx::query(&statement).execute(pool).await?;
    }
    sqlx::query("INSERT INTO schema_version (version) VALUES (8) ON CONFLICT (version) DO NOTHING")
        .execute(pool)
        .await?;

    for stmt in [
        "CREATE INDEX IF NOT EXISTS idx_findings_detector ON findings(detector)",
        "CREATE INDEX IF NOT EXISTS idx_dismissed_detector ON dismissed_findings(detector)",
        "CREATE INDEX IF NOT EXISTS idx_webhook_received_at ON webhook_deliveries(received_at)",
        "CREATE INDEX IF NOT EXISTS idx_review_jobs_status_updated ON review_jobs(status, updated_at)",
        "CREATE TABLE IF NOT EXISTS agent_events (
            id BIGSERIAL PRIMARY KEY,
            ts TEXT NOT NULL DEFAULT (NOW()::text),
            agent TEXT NOT NULL,
            event_type TEXT NOT NULL,
            model TEXT,
            tokens_in INTEGER,
            tokens_out INTEGER,
            cost_usd_est DOUBLE PRECISION,
            latency_ms INTEGER,
            outcome TEXT,
            payload TEXT
        )",
        "CREATE INDEX IF NOT EXISTS idx_agent_events_ts ON agent_events(ts)",
        "CREATE INDEX IF NOT EXISTS idx_agent_events_type_ts ON agent_events(event_type, ts)",
    ] {
        sqlx::query(stmt).execute(pool).await?;
    }
    sqlx::query("INSERT INTO schema_version (version) VALUES (9) ON CONFLICT (version) DO NOTHING")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO schema_version (version) VALUES (10) ON CONFLICT (version) DO NOTHING",
    )
    .execute(pool)
    .await?;
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
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        statements.push(trimmed);
    }
    statements
}
