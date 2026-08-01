# Operations — backup & restore

State lives in **SQLite** (default) or **Postgres** (`DATABASE_URL=postgres://…`). Treat the database and your secret store as the recovery unit.

## What to back up

| Target | Contents |
| --- | --- |
| `CODASAURUS_DATA_DIR` / Docker `/data` | `codasaurus.db` (+ `-wal` / `-shm` if copying a hot file naively) |
| Postgres volume / managed instance | Full database |
| Secrets store / vault | `GITHUB_APP_*`, webhook secret, LLM keys, `OIDC_*`, admin password |

In that DB: repos, reviews, findings, dismissals, learned rules, sessions, job queue, and dashboard config keys.

## SQLite — online backup

```bash
# Prefer the backup API over raw file copy while the process is running
sqlite3 /data/codasaurus.db "PRAGMA wal_checkpoint(TRUNCATE);"
sqlite3 /data/codasaurus.db ".backup '/backup/codasaurus-$(date +%Y%m%d).db'"
```

**Restore**

1. Stop Codasaurus.
2. Replace `codasaurus.db` with the backup file.
3. Remove stale WAL sidecars: `rm -f codasaurus.db-wal codasaurus.db-shm`.
4. Start Codasaurus.

## Postgres

```bash
pg_dump "$DATABASE_URL" -Fc -f "codasaurus-$(date +%Y%m%d).dump"

# restore (example with Compose overlay)
docker compose -f docker-compose.yml -f docker-compose.postgres.yml stop codasaurus
pg_restore -d "$DATABASE_URL" --clean --if-exists codasaurus-YYYYMMDD.dump
docker compose -f docker-compose.yml -f docker-compose.postgres.yml start codasaurus
```

## Multi-replica / HA

| Backend | Guidance |
| --- | --- |
| SQLite | Single writer only — one Codasaurus replica |
| Postgres | HA path: `FOR UPDATE SKIP LOCKED` + SHA leases for review ownership |

Compose overlay:

```bash
docker compose -f docker-compose.yml -f docker-compose.postgres.yml up -d
```

## Health & metrics

```bash
curl -sf http://localhost:3000/health
curl -sf http://localhost:3000/metrics | head
```

`/health` JSON includes `status`, `db`, `data_dir`, `version`, `egress_profile` (`full` · `byok-only` · `offline`), and `network` flags for LLM / registries / OSV.

Docker healthcheck runs `codasaurus health --port 3000`.

## Disaster checklist

1. Restore DB from last known-good backup.
2. Restore secrets (App PEM + webhook secret must match the GitHub App).
3. Confirm `PUBLIC_URL` / reverse proxy still match webhook URLs.
4. `curl /health` → open a test PR → confirm a walkthrough comment lands.
