//! PostgreSQL schema migrations (sole backend).

use sqlx::PgPool;

const PG_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version BIGINT PRIMARY KEY,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS app_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
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
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_repos_owner ON repos(owner);
CREATE INDEX IF NOT EXISTS idx_repos_active ON repos(active) WHERE active = TRUE;
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
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reviews_status ON reviews(status);
CREATE INDEX IF NOT EXISTS idx_reviews_created ON reviews(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reviews_repo_created ON reviews(repo_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reviews_repo_status ON reviews(repo_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_repo_pr_sha
    ON reviews(repo_id, pr_number, pr_head_sha);

CREATE TABLE IF NOT EXISTS findings (
    id BIGSERIAL PRIMARY KEY,
    review_id BIGINT NOT NULL REFERENCES reviews(id) ON DELETE CASCADE,
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
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_findings_review ON findings(review_id);
CREATE INDEX IF NOT EXISTS idx_findings_severity ON findings(severity);
CREATE INDEX IF NOT EXISTS idx_findings_detector ON findings(detector);

CREATE TABLE IF NOT EXISTS dismissals (
    id BIGSERIAL PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    dismissed_by TEXT,
    reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(fingerprint)
);

CREATE TABLE IF NOT EXISTS audit_log (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL,
    actor TEXT,
    target_type TEXT,
    target_id BIGINT,
    metadata_json TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_event ON audit_log(event_type);
CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_log(created_at DESC);

CREATE TABLE IF NOT EXISTS users (
    id BIGSERIAL PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL DEFAULT '',
    role TEXT NOT NULL DEFAULT 'owner' CHECK (role IN ('owner', 'maintainer', 'viewer')),
    auth_provider TEXT NOT NULL DEFAULT 'local',
    is_bootstrap BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS sessions (
    token TEXT PRIMARY KEY,
    email TEXT NOT NULL REFERENCES users(email) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);

CREATE TABLE IF NOT EXISTS invites (
    id BIGSERIAL PRIMARY KEY,
    token_hash TEXT UNIQUE NOT NULL,
    email TEXT,
    role TEXT NOT NULL CHECK (role IN ('owner', 'maintainer', 'viewer')),
    created_by TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_invites_pending
    ON invites (expires_at ASC) WHERE accepted_at IS NULL;

CREATE TABLE IF NOT EXISTS webhook_deliveries (
    delivery_id TEXT PRIMARY KEY,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_webhook_received_at ON webhook_deliveries(received_at);

CREATE TABLE IF NOT EXISTS review_comments (
    repo_pr TEXT PRIMARY KEY,
    comment_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS reviewed_commits (
    repo_pr TEXT PRIMARY KEY,
    head_sha TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'completed'
        CHECK (status IN ('in_progress', 'completed')),
    lease_owner TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reviewed_commits_in_progress
    ON reviewed_commits (created_at ASC) WHERE status = 'in_progress';

CREATE TABLE IF NOT EXISTS dismissed_findings (
    fingerprint TEXT PRIMARY KEY,
    detector TEXT NOT NULL,
    file TEXT NOT NULL,
    line INTEGER NOT NULL,
    message TEXT NOT NULL,
    dismissed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    pr_number BIGINT,
    dismissed_by TEXT,
    is_maintainer BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE IF NOT EXISTS learned_rules (
    id TEXT PRIMARY KEY,
    detector TEXT NOT NULL,
    file_pattern TEXT,
    message_pattern TEXT,
    action TEXT NOT NULL DEFAULT 'ignore',
    reason TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

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
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_review_jobs_repo_pr_sha
    ON review_jobs(repo, pr_number, head_sha);
CREATE INDEX IF NOT EXISTS idx_review_jobs_pending_created
    ON review_jobs (created_at ASC) WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_review_jobs_running_updated
    ON review_jobs (updated_at ASC) WHERE status = 'running';
CREATE INDEX IF NOT EXISTS idx_review_jobs_finished_updated
    ON review_jobs (updated_at ASC) WHERE status IN ('done', 'failed');

CREATE TABLE IF NOT EXISTS agent_events (
    id BIGSERIAL PRIMARY KEY,
    ts TIMESTAMPTZ NOT NULL DEFAULT NOW(),
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

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    for statement in split_sql(PG_SCHEMA) {
        sqlx::query(&statement).execute(pool).await?;
    }
    sqlx::query("INSERT INTO schema_version (version) VALUES (8) ON CONFLICT (version) DO NOTHING")
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO schema_version (version) VALUES (9) ON CONFLICT (version) DO NOTHING")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO schema_version (version) VALUES (10) ON CONFLICT (version) DO NOTHING",
    )
    .execute(pool)
    .await?;

    migrate_v11_timestamptz_and_indexes(pool).await?;
    migrate_v12_roles_and_invites(pool).await?;
    migrate_v13_bootstrap_owner(pool).await?;
    migrate_v14_repo_scoped_learning(pool).await?;
    migrate_v15_invites_email_index(pool).await?;
    migrate_v16_dismissal_provenance(pool).await?;
    migrate_v17_baseline_and_gates(pool).await?;
    migrate_v18_confidence(pool).await?;
    migrate_v19_symbol_index(pool).await?;
    migrate_v20_pre_merge_checks(pool).await?;
    migrate_v21_learning_rule_status(pool).await?;
    Ok(())
}

/// v21: rule lifecycle (suggested → approved → archived) + source count.
async fn migrate_v21_learning_rule_status(pool: &PgPool) -> Result<(), sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;
    if current.unwrap_or(0) >= 21 {
        return Ok(());
    }
    let _ = sqlx::query(
        r#"
        ALTER TABLE learned_rules
            ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'approved',
            ADD COLUMN IF NOT EXISTS source_count INTEGER NOT NULL DEFAULT 0,
            ADD COLUMN IF NOT EXISTS approved_at TIMESTAMPTZ,
            ADD COLUMN IF NOT EXISTS archived_at TIMESTAMPTZ
        "#,
    )
    .execute(pool)
    .await;
    Ok(())
}

/// v20: per-PR natural-language pre-merge check runs (Phase 5).
async fn migrate_v20_pre_merge_checks(pool: &PgPool) -> Result<(), sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;
    if current.unwrap_or(0) >= 20 {
        return Ok(());
    }
    let _ = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pre_merge_check_runs (
            repo_full_name TEXT NOT NULL,
            pr_number INTEGER NOT NULL,
            check_name TEXT NOT NULL,
            mode TEXT NOT NULL,
            status TEXT NOT NULL,
            reasoning TEXT,
            evaluated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            PRIMARY KEY (repo_full_name, pr_number, check_name)
        )
        "#,
    )
    .execute(pool)
    .await;
    Ok(())
}

/// v19: whole-repo symbol graph index (tree-sitter output persisted).
async fn migrate_v19_symbol_index(pool: &PgPool) -> Result<(), sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;
    if current.unwrap_or(0) >= 19 {
        return Ok(());
    }

    let _ = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS repo_symbols (
            repo_full_name TEXT NOT NULL,
            file_path TEXT NOT NULL,
            symbol_name TEXT NOT NULL,
            kind TEXT NOT NULL,
            signature TEXT,
            line INTEGER,
            PRIMARY KEY (repo_full_name, file_path, symbol_name, line)
        )
        "#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_repo_symbols_lookup ON repo_symbols(repo_full_name, file_path)",
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS repo_edges (
            repo_full_name TEXT NOT NULL,
            from_symbol TEXT NOT NULL,
            to_symbol TEXT NOT NULL,
            edge_kind TEXT NOT NULL,
            PRIMARY KEY (repo_full_name, from_symbol, to_symbol, edge_kind)
        )
        "#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_repo_edges_lookup ON repo_edges(repo_full_name, from_symbol)",
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_repo_edges_target ON repo_edges(repo_full_name, to_symbol)",
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS index_status (
            repo_full_name TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            built_at TIMESTAMPTZ,
            error TEXT
        )
        "#,
    )
    .execute(pool)
    .await;

    sqlx::query(
        "INSERT INTO schema_version (version) VALUES (19) ON CONFLICT (version) DO NOTHING",
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// v18: per-finding confidence (0-5) + LLM judge rationale.
async fn migrate_v18_confidence(pool: &PgPool) -> Result<(), sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;
    if current.unwrap_or(0) >= 18 {
        return Ok(());
    }

    let _ = sqlx::query("ALTER TABLE findings ADD COLUMN IF NOT EXISTS confidence INTEGER")
        .execute(pool)
        .await;

    let _ = sqlx::query("ALTER TABLE findings ADD COLUMN IF NOT EXISTS judge_rationale TEXT")
        .execute(pool)
        .await;

    sqlx::query(
        "INSERT INTO schema_version (version) VALUES (18) ON CONFLICT (version) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// v17: baseline (new-code filtering) + quality gates.
async fn migrate_v17_baseline_and_gates(pool: &PgPool) -> Result<(), sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;
    if current.unwrap_or(0) >= 17 {
        return Ok(());
    }

    let _ = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS finding_baseline (
            repo_full_name TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            resolved_at TIMESTAMPTZ,
            PRIMARY KEY (repo_full_name, fingerprint)
        )
        "#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_finding_baseline_repo ON finding_baseline(repo_full_name)",
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pr_diff_lines (
            repo_full_name TEXT NOT NULL,
            pr_number BIGINT NOT NULL,
            file_path TEXT NOT NULL,
            line INTEGER NOT NULL,
            PRIMARY KEY (repo_full_name, pr_number, file_path, line)
        )
        "#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_pr_diff_lines_lookup ON pr_diff_lines(repo_full_name, pr_number, file_path)",
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS quality_gates (
            repo_full_name TEXT PRIMARY KEY,
            gate_json TEXT NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await;

    sqlx::query(
        "INSERT INTO schema_version (version) VALUES (17) ON CONFLICT (version) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// v16: track PR + actor on dismissals so auto-learn requires distinct PRs or a maintainer.
async fn migrate_v16_dismissal_provenance(pool: &PgPool) -> Result<(), sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;
    if current.unwrap_or(0) >= 16 {
        return Ok(());
    }

    let _ = sqlx::query("ALTER TABLE dismissed_findings ADD COLUMN IF NOT EXISTS pr_number BIGINT")
        .execute(pool)
        .await;
    let _ =
        sqlx::query("ALTER TABLE dismissed_findings ADD COLUMN IF NOT EXISTS dismissed_by TEXT")
            .execute(pool)
            .await;
    let _ = sqlx::query(
        "ALTER TABLE dismissed_findings ADD COLUMN IF NOT EXISTS is_maintainer BOOLEAN NOT NULL DEFAULT FALSE",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_dismissed_detector_pr ON dismissed_findings (detector, pr_number)",
    )
    .execute(pool)
    .await;

    sqlx::query(
        "INSERT INTO schema_version (version) VALUES (16) ON CONFLICT (version) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// v15: case-insensitive pending-invite lookup by email.
async fn migrate_v15_invites_email_index(pool: &PgPool) -> Result<(), sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;
    if current.unwrap_or(0) >= 15 {
        return Ok(());
    }

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_invites_email_pending ON invites (lower(email)) WHERE accepted_at IS NULL",
    )
    .execute(pool)
    .await;

    sqlx::query(
        "INSERT INTO schema_version (version) VALUES (15) ON CONFLICT (version) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// v14: optional repo scope on dismissals and learned rules.
async fn migrate_v14_repo_scoped_learning(pool: &PgPool) -> Result<(), sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;
    if current.unwrap_or(0) >= 14 {
        return Ok(());
    }

    let _ =
        sqlx::query("ALTER TABLE dismissed_findings ADD COLUMN IF NOT EXISTS repo_full_name TEXT")
            .execute(pool)
            .await;
    let _ = sqlx::query("ALTER TABLE learned_rules ADD COLUMN IF NOT EXISTS repo_full_name TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_dismissed_repo ON dismissed_findings(repo_full_name)",
    )
    .execute(pool)
    .await;
    let _ =
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_learned_repo ON learned_rules(repo_full_name)")
            .execute(pool)
            .await;

    sqlx::query(
        "INSERT INTO schema_version (version) VALUES (14) ON CONFLICT (version) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// v13: mark the first/onboarding account as bootstrap (instance superuser).
async fn migrate_v13_bootstrap_owner(pool: &PgPool) -> Result<(), sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;
    if current.unwrap_or(0) >= 13 {
        return Ok(());
    }

    let _ = sqlx::query(
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS is_bootstrap BOOLEAN NOT NULL DEFAULT FALSE",
    )
    .execute(pool)
    .await;

    // Existing installs: earliest owner becomes the bootstrap account.
    let _ = sqlx::query(
        r#"
        UPDATE users SET is_bootstrap = TRUE
        WHERE id = (
            SELECT id FROM users
            WHERE role IN ('owner', 'admin')
            ORDER BY created_at ASC, id ASC
            LIMIT 1
        )
        AND NOT EXISTS (SELECT 1 FROM users WHERE is_bootstrap = TRUE)
        "#,
    )
    .execute(pool)
    .await;

    sqlx::query(
        "INSERT INTO schema_version (version) VALUES (13) ON CONFLICT (version) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// v12: admin/viewer → owner/maintainer/viewer + invites table.
async fn migrate_v12_roles_and_invites(pool: &PgPool) -> Result<(), sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;
    if current.unwrap_or(0) >= 12 {
        return Ok(());
    }

    // Drop old CHECK, migrate roles, re-add CHECK for new role set.
    let _ = sqlx::query("ALTER TABLE users DROP CONSTRAINT IF EXISTS users_role_check")
        .execute(pool)
        .await;
    sqlx::query("UPDATE users SET role = 'owner' WHERE role = 'admin'")
        .execute(pool)
        .await?;
    // Any unexpected role becomes viewer.
    sqlx::query(
        "UPDATE users SET role = 'viewer' WHERE role NOT IN ('owner', 'maintainer', 'viewer')",
    )
    .execute(pool)
    .await?;
    sqlx::query("ALTER TABLE users ALTER COLUMN role SET DEFAULT 'owner'")
        .execute(pool)
        .await?;
    let _ = sqlx::query(
        "ALTER TABLE users ADD CONSTRAINT users_role_check CHECK (role IN ('owner', 'maintainer', 'viewer'))",
    )
    .execute(pool)
    .await;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS invites (
            id BIGSERIAL PRIMARY KEY,
            token_hash TEXT UNIQUE NOT NULL,
            email TEXT,
            role TEXT NOT NULL CHECK (role IN ('owner', 'maintainer', 'viewer')),
            created_by TEXT NOT NULL,
            expires_at TIMESTAMPTZ NOT NULL,
            accepted_at TIMESTAMPTZ,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await?;
    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_invites_pending ON invites (expires_at ASC) WHERE accepted_at IS NULL",
    )
    .execute(pool)
    .await;

    sqlx::query(
        "INSERT INTO schema_version (version) VALUES (12) ON CONFLICT (version) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// v11: TEXT → TIMESTAMPTZ on existing installs + partial queue/lease indexes.
async fn migrate_v11_timestamptz_and_indexes(pool: &PgPool) -> Result<(), sqlx::Error> {
    let current: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await?;
    if current.unwrap_or(0) >= 11 {
        return Ok(());
    }

    // Convert timestamp columns that may still be TEXT from older installs.
    for stmt in [
        alter_ts("schema_version", "applied_at", false),
        alter_ts("app_config", "updated_at", false),
        alter_ts("repos", "created_at", false),
        alter_ts("repos", "updated_at", false),
        alter_ts("reviews", "started_at", true),
        alter_ts("reviews", "completed_at", true),
        alter_ts("reviews", "created_at", false),
        alter_ts("findings", "created_at", false),
        alter_ts("dismissals", "created_at", false),
        alter_ts("audit_log", "created_at", false),
        alter_ts("users", "created_at", false),
        alter_ts("sessions", "created_at", false),
        alter_ts("sessions", "expires_at", false),
        alter_ts("webhook_deliveries", "received_at", false),
        alter_ts("review_comments", "created_at", false),
        alter_ts("reviewed_commits", "created_at", false),
        alter_ts("dismissed_findings", "dismissed_at", false),
        alter_ts("learned_rules", "created_at", false),
        alter_ts("review_jobs", "created_at", false),
        alter_ts("review_jobs", "updated_at", false),
        alter_ts("agent_events", "ts", false),
    ] {
        let _ = sqlx::query(stmt).execute(pool).await;
    }

    for stmt in [
        "DROP INDEX IF EXISTS idx_reviews_repo",
        "DROP INDEX IF EXISTS idx_findings_fingerprint",
        "DROP INDEX IF EXISTS idx_findings_file",
        "DROP INDEX IF EXISTS idx_dismissed_fingerprint",
        "DROP INDEX IF EXISTS idx_review_jobs_status_created",
        "DROP INDEX IF EXISTS idx_review_jobs_status_updated",
        "DROP INDEX IF EXISTS idx_repos_active",
        "CREATE INDEX IF NOT EXISTS idx_repos_active ON repos(active) WHERE active = TRUE",
        "CREATE INDEX IF NOT EXISTS idx_review_jobs_pending_created ON review_jobs (created_at ASC) WHERE status = 'pending'",
        "CREATE INDEX IF NOT EXISTS idx_review_jobs_running_updated ON review_jobs (updated_at ASC) WHERE status = 'running'",
        "CREATE INDEX IF NOT EXISTS idx_review_jobs_finished_updated ON review_jobs (updated_at ASC) WHERE status IN ('done', 'failed')",
        "CREATE INDEX IF NOT EXISTS idx_reviewed_commits_in_progress ON reviewed_commits (created_at ASC) WHERE status = 'in_progress'",
        // findings.review_id cascade for review cleanup (ignore if already set)
        "ALTER TABLE findings DROP CONSTRAINT IF EXISTS findings_review_id_fkey",
        "ALTER TABLE findings ADD CONSTRAINT findings_review_id_fkey FOREIGN KEY (review_id) REFERENCES reviews(id) ON DELETE CASCADE",
    ] {
        let _ = sqlx::query(stmt).execute(pool).await;
    }

    sqlx::query(
        "INSERT INTO schema_version (version) VALUES (11) ON CONFLICT (version) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(())
}

fn alter_ts(table: &str, column: &str, nullable: bool) -> &'static str {
    // Static strings only — callers pass fixed identifiers.
    match (table, column, nullable) {
        ("schema_version", "applied_at", false) => {
            "ALTER TABLE schema_version ALTER COLUMN applied_at DROP DEFAULT, ALTER COLUMN applied_at TYPE TIMESTAMPTZ USING applied_at::timestamptz, ALTER COLUMN applied_at SET DEFAULT NOW()"
        }
        ("app_config", "updated_at", false) => {
            "ALTER TABLE app_config ALTER COLUMN updated_at DROP DEFAULT, ALTER COLUMN updated_at TYPE TIMESTAMPTZ USING updated_at::timestamptz, ALTER COLUMN updated_at SET DEFAULT NOW()"
        }
        ("repos", "created_at", false) => {
            "ALTER TABLE repos ALTER COLUMN created_at DROP DEFAULT, ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at::timestamptz, ALTER COLUMN created_at SET DEFAULT NOW()"
        }
        ("repos", "updated_at", false) => {
            "ALTER TABLE repos ALTER COLUMN updated_at DROP DEFAULT, ALTER COLUMN updated_at TYPE TIMESTAMPTZ USING updated_at::timestamptz, ALTER COLUMN updated_at SET DEFAULT NOW()"
        }
        ("reviews", "started_at", true) => {
            "ALTER TABLE reviews ALTER COLUMN started_at TYPE TIMESTAMPTZ USING NULLIF(started_at, '')::timestamptz"
        }
        ("reviews", "completed_at", true) => {
            "ALTER TABLE reviews ALTER COLUMN completed_at TYPE TIMESTAMPTZ USING NULLIF(completed_at, '')::timestamptz"
        }
        ("reviews", "created_at", false) => {
            "ALTER TABLE reviews ALTER COLUMN created_at DROP DEFAULT, ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at::timestamptz, ALTER COLUMN created_at SET DEFAULT NOW()"
        }
        ("findings", "created_at", false) => {
            "ALTER TABLE findings ALTER COLUMN created_at DROP DEFAULT, ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at::timestamptz, ALTER COLUMN created_at SET DEFAULT NOW()"
        }
        ("dismissals", "created_at", false) => {
            "ALTER TABLE dismissals ALTER COLUMN created_at DROP DEFAULT, ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at::timestamptz, ALTER COLUMN created_at SET DEFAULT NOW()"
        }
        ("audit_log", "created_at", false) => {
            "ALTER TABLE audit_log ALTER COLUMN created_at DROP DEFAULT, ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at::timestamptz, ALTER COLUMN created_at SET DEFAULT NOW()"
        }
        ("users", "created_at", false) => {
            "ALTER TABLE users ALTER COLUMN created_at DROP DEFAULT, ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at::timestamptz, ALTER COLUMN created_at SET DEFAULT NOW()"
        }
        ("sessions", "created_at", false) => {
            "ALTER TABLE sessions ALTER COLUMN created_at DROP DEFAULT, ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at::timestamptz, ALTER COLUMN created_at SET DEFAULT NOW()"
        }
        ("sessions", "expires_at", false) => {
            "ALTER TABLE sessions ALTER COLUMN expires_at TYPE TIMESTAMPTZ USING expires_at::timestamptz"
        }
        ("webhook_deliveries", "received_at", false) => {
            "ALTER TABLE webhook_deliveries ALTER COLUMN received_at DROP DEFAULT, ALTER COLUMN received_at TYPE TIMESTAMPTZ USING received_at::timestamptz, ALTER COLUMN received_at SET DEFAULT NOW()"
        }
        ("review_comments", "created_at", false) => {
            "ALTER TABLE review_comments ALTER COLUMN created_at DROP DEFAULT, ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at::timestamptz, ALTER COLUMN created_at SET DEFAULT NOW()"
        }
        ("reviewed_commits", "created_at", false) => {
            "ALTER TABLE reviewed_commits ALTER COLUMN created_at DROP DEFAULT, ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at::timestamptz, ALTER COLUMN created_at SET DEFAULT NOW()"
        }
        ("dismissed_findings", "dismissed_at", false) => {
            "ALTER TABLE dismissed_findings ALTER COLUMN dismissed_at DROP DEFAULT, ALTER COLUMN dismissed_at TYPE TIMESTAMPTZ USING dismissed_at::timestamptz, ALTER COLUMN dismissed_at SET DEFAULT NOW()"
        }
        ("learned_rules", "created_at", false) => {
            "ALTER TABLE learned_rules ALTER COLUMN created_at DROP DEFAULT, ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at::timestamptz, ALTER COLUMN created_at SET DEFAULT NOW()"
        }
        ("review_jobs", "created_at", false) => {
            "ALTER TABLE review_jobs ALTER COLUMN created_at DROP DEFAULT, ALTER COLUMN created_at TYPE TIMESTAMPTZ USING created_at::timestamptz, ALTER COLUMN created_at SET DEFAULT NOW()"
        }
        ("review_jobs", "updated_at", false) => {
            "ALTER TABLE review_jobs ALTER COLUMN updated_at DROP DEFAULT, ALTER COLUMN updated_at TYPE TIMESTAMPTZ USING updated_at::timestamptz, ALTER COLUMN updated_at SET DEFAULT NOW()"
        }
        ("agent_events", "ts", false) => {
            "ALTER TABLE agent_events ALTER COLUMN ts DROP DEFAULT, ALTER COLUMN ts TYPE TIMESTAMPTZ USING ts::timestamptz, ALTER COLUMN ts SET DEFAULT NOW()"
        }
        _ => "SELECT 1",
    }
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
