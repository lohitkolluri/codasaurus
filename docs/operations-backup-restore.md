# Operations — backup & restore

State lives in **PostgreSQL** (`DATABASE_URL=postgres://…`). Treat the database and your secret store as the recovery unit. Schema and pool details: [database.md](database.md).

## What to back up

| Target                                 | Contents                                                           |
| -------------------------------------- | ------------------------------------------------------------------ |
| Postgres volume / managed instance     | Full database (reviews, jobs, sessions, learning, config)          |
| `CODASAURUS_DATA_DIR` / Docker `/data` | Non-DB artifacts only (if any)                                     |
| Secrets store / vault                  | `GITHUB_APP_*`, webhook secret, LLM keys, `OIDC_*`, admin password |

In that DB: repos, reviews, findings, dismissals, learned rules, sessions, job queue, `agent_events`, and dashboard config keys.

## Postgres backup & restore

```bash
pg_dump "$DATABASE_URL" -Fc -f "codasaurus-$(date +%Y%m%d).dump"

# restore (example with Compose)
docker compose stop codasaurus
pg_restore -d "$DATABASE_URL" --clean --if-exists codasaurus-YYYYMMDD.dump
docker compose start codasaurus
```

## Multi-replica / HA

Postgres is the HA path: `FOR UPDATE SKIP LOCKED` on the job queue plus SHA leases for review ownership. Point every replica at the same `DATABASE_URL`.

```bash
docker compose up -d
```

## Health & metrics

```bash
curl -sf http://localhost:3000/health
curl -sf http://localhost:3000/metrics | head
```

`/health` JSON includes `status`, `db`, `data_dir`, `version`, `egress_profile` (`full` · `byok-only` · `offline`), and `network` flags for LLM / registries / OSV.

Docker healthcheck runs `codasaurus health --port 3000`.

## Disaster checklist

1. Restore Postgres from last known-good backup.
2. Restore secrets (App PEM + webhook secret must match the GitHub App).
3. Confirm `PUBLIC_URL` / reverse proxy still match webhook URLs.
4. `curl /health` → open a test PR → confirm a walkthrough comment lands.
