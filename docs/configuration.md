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

| Variable | Required | Description |
| --- | --- | --- |
| `DATABASE_URL` | no | Default SQLite file URL, e.g. `sqlite:///data/codasaurus.db?mode=rwc` or `postgres://…` |
| `CODASAURUS_DATA_DIR` | no | Data directory (Docker: `/data`) |
| `PORT` / `--port` | no | Listen port (default `3000`) |
| `PUBLIC_URL` | recommended in prod | Canonical HTTPS origin for GitHub manifest callbacks |

### GitHub App

| Variable | Required | Description |
| --- | --- | --- |
| `GITHUB_APP_ID` | yes\* | Numeric App ID |
| `GITHUB_APP_PRIVATE_KEY` | yes\* | PEM with literal newlines |
| `GITHUB_APP_PRIVATE_KEY_B64` | alt | Base64-encoded PEM (compose-friendly) |
| `GITHUB_WEBHOOK_SECRET` | yes\* | Shared webhook secret |

\*Required for live reviews unless the wizard already stored them in the DB.

### LLM (optional BYOK)

| Variable | Description |
| --- | --- |
| `OPENROUTER_API_KEY` | OpenRouter (or stored as dashboard key) |
| `CODASAURUS_BASE_URL` | OpenAI-compatible base, e.g. `http://localhost:11434/v1` |
| `CODASAURUS_MODEL` | Model id |
| `CODASAURUS_OFFLINE` | `1` / `true` — fail-closed: no LLM socket; registry/OSV cache-only |

Dashboard **Settings → LLM** and setup wizard write `llm_provider`, `llm_model`, `llm_base_url`, `openrouter_api_key`.

### Auth / SSO

| Variable | Description |
| --- | --- |
| `OIDC_ISSUER` | OIDC issuer URL |
| `OIDC_CLIENT_ID` | Client id |
| `OIDC_CLIENT_SECRET` | Client secret |
| `PUBLIC_URL` | Must match IdP redirect URIs |

Local admin users are created in the [onboarding wizard](setup-onboarding.md).

### Runtime tuning

| Variable | Description |
| --- | --- |
| `CODASAURUS_MAX_CONCURRENT_REVIEWS` | Review worker concurrency (compose default `4`) |
| `CODASAURUS_CONFIG` | Path to TOML config override |
| `CODASAURUS_SKIP_FRONTEND_BUILD` | `1` in tests to skip embedding SPA build |

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

| `egress_profile` | Meaning |
| --- | --- |
| `offline` | `CODASAURUS_OFFLINE` or dashboard `offline_mode` — no LLM; registries/OSV fail-closed or cache-only |
| `byok-only` | LLM endpoint/key configured; Tier‑1 network allowed |
| `full` | Not offline; no LLM configured (Tier‑1 only) |

## Docker

```bash
docker compose up
docker compose -f docker-compose.yml -f docker-compose.postgres.yml up
```

See [operations-backup-restore.md](operations-backup-restore.md) for volumes and HA notes.
