# Configuration

Codasaurus merges **environment variables**, optional **TOML**, and **dashboard / DB** settings. Precedence for GitHub App credentials: env → DB (wizard). Detector toggles and policy prefer DB overlays when present.

## Process commands

```text
codasaurus serve [--host 0.0.0.0] [--port 3000]
codasaurus health [--port 3000]
codasaurus version
```

## Environment variables

### Core

| Variable              | Required            | Description                                                                             |
| --------------------- | ------------------- | --------------------------------------------------------------------------------------- |
| `DATABASE_URL`        | no                  | Default SQLite file URL, e.g. `sqlite:///data/codasaurus.db?mode=rwc` or `postgres://…` |
| `CODASAURUS_DATA_DIR` | no                  | Data directory (Docker: `/data`)                                                        |
| `PORT` / `--port`     | no                  | Listen port (default `3000`)                                                            |
| `PUBLIC_URL`          | recommended in prod | Canonical HTTPS origin for GitHub manifest callbacks                                    |

### GitHub App

| Variable                     | Required | Description                           |
| ---------------------------- | -------- | ------------------------------------- |
| `GITHUB_APP_ID`              | yes\*    | Numeric App ID                        |
| `GITHUB_APP_PRIVATE_KEY`     | yes\*    | PEM with literal newlines             |
| `GITHUB_APP_PRIVATE_KEY_B64` | alt      | Base64-encoded PEM (compose-friendly) |
| `GITHUB_WEBHOOK_SECRET`      | yes\*    | Shared webhook secret                 |

\*Required for live reviews unless the wizard already stored them in the DB.

### LLM (optional BYOK)

| Variable                            | Description                                                                    |
| ----------------------------------- | ------------------------------------------------------------------------------ |
| `OPENROUTER_API_KEY`                | OpenRouter (or stored as dashboard key)                                        |
| `CODASAURUS_BASE_URL`               | OpenAI-compatible base, e.g. `http://localhost:11434/v1`                       |
| `CODASAURUS_MODEL`                  | Strong model for structured `review_diff`                                      |
| `CODASAURUS_MODEL_CHEAP`            | Cheap model for summarize / describe / ask / changelog (defaults from primary) |
| `CODASAURUS_OFFLINE`                | `1` / `true` — fail-closed: no LLM socket; registry/OSV cache-only             |
| `CODASAURUS_MAX_LLM_DIFF_CHARS`     | Cap chars sent to `review_diff` (default `8000`)                               |
| `CODASAURUS_AUTO_IMPROVE_MAX_FILES` | Skip auto improve above this file count (default `40`)                         |
| `CODASAURUS_AUTO_IMPROVE_MAX_DIFF`  | Cap aggregated patch chars for auto improve (default `24000`)                  |

Dashboard **Settings → LLM** writes `llm_provider`, `llm_model`, `llm_model_cheap`, `llm_base_url`, `openrouter_api_key`.

### LLM cost controls

| Knob                                        | Default             | Effect                                                            |
| ------------------------------------------- | ------------------- | ----------------------------------------------------------------- |
| Repo `config_json.auto_review_diff`         | **off (opt-in)**    | Webhook auto `review_diff` (largest cost)                         |
| Skip when Tier-1 blocks                     | always              | No auto improve if review already holds                           |
| Skip lockfile / vendor / generated-only PRs | always              | No auto improve on low-signal paths                               |
| Hunk filter                                 | always              | Lockfiles, `vendor/`, `dist/`, maps, binaries stripped before LLM |
| Two-tier models                             | on                  | Strong for `review_diff`; cheap for text helpers                  |
| Prompt caching                              | Claude / OpenRouter | `cache_control` on stable system prefixes when supported          |

`/metrics` exposes `codasaurus_llm_spend_usd_estimate` (rough process-local estimate). Dashboard **stats** includes `llm.spend_usd_estimate`.

### Auth / SSO

| Variable             | Description                  |
| -------------------- | ---------------------------- |
| `OIDC_ISSUER`        | OIDC issuer URL              |
| `OIDC_CLIENT_ID`     | Client id                    |
| `OIDC_CLIENT_SECRET` | Client secret                |
| `PUBLIC_URL`         | Must match IdP redirect URIs |

Local admin users are created in the [onboarding wizard](setup-onboarding.md).

### Runtime tuning

| Variable                            | Description                                     |
| ----------------------------------- | ----------------------------------------------- |
| `CODASAURUS_MAX_CONCURRENT_REVIEWS` | Review worker concurrency (compose default `4`) |
| `CODASAURUS_CONFIG`                 | Path to TOML config override                    |
| `CODASAURUS_SKIP_FRONTEND_BUILD`    | `1` in tests to skip embedding SPA build        |

### Caching, TTL, indexes

| Layer            | Behavior                                                                                                                                                                                                               |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Registry / OSV   | In-process HashMap, default TTL **3600s** (`[registry] cache_ttl_secs`). Soft-fail cache **120s** on network errors. Prefetch up to 200 packages/PR. Metrics: `codasaurus_registry_cache_*`, `codasaurus_osv_cache_*`. |
| GitHub Contents  | In-process ETag + body cache (1h, 2k entries) with `If-None-Match` so 304s spare primary rate limit. Metrics: `codasaurus_github_cache_*`.                                                                             |
| LLM prompt cache | Provider-side `cache_control: ephemeral` on stable system prompts (Claude / OpenRouter).                                                                                                                               |
| Webhook dedup    | `webhook_deliveries` PK; rows pruned after **14 days**.                                                                                                                                                                |
| Sessions         | **7 days**; purged on lookup and by worker maintenance.                                                                                                                                                                |
| Review jobs      | Terminal `done`/`failed` purged after **30 days**.                                                                                                                                                                     |
| DB indexes       | `findings(detector)`, `dismissed_findings(detector)`, `webhook_deliveries(received_at)`, `review_jobs(status, updated_at)` (schema v9).                                                                                |

### Pattern-first (reduce LLM)

Tier‑1 detectors catch secrets, IaC, hallucinated imports, phantom deps, OSV, **stale APIs** (on by default), and **risky patterns** (`eval`, XSS sinks, SQL concat, TLS skip, …) without an LLM. Lockfile/vendor/generated-only PRs skip Contents fan-out, registry prefetch, and LLM summary/improve.

## TOML sketch

```toml
[checks]
hallucinated_imports = true
phantom_deps = true
vulnerabilities = true
secrets = true
iac = true
```

Per-repo overlays and policy packs are edited in the dashboard (`config_json`, forbidden paths, severity caps, auto-labels, Check Runs, `@codasaurus fix`).

## Offline / egress profiles

`/health` reports:

| `egress_profile` | Meaning                                                                                             |
| ---------------- | --------------------------------------------------------------------------------------------------- |
| `offline`        | `CODASAURUS_OFFLINE` or dashboard `offline_mode` — no LLM; registries/OSV fail-closed or cache-only |
| `byok-only`      | LLM endpoint/key configured; Tier‑1 network allowed                                                 |
| `full`           | Not offline; no LLM configured (Tier‑1 only)                                                        |

## Docker

```bash
docker compose up
docker compose -f docker-compose.yml -f docker-compose.postgres.yml up
```

See [operations-backup-restore.md](operations-backup-restore.md) for volumes and HA notes.
