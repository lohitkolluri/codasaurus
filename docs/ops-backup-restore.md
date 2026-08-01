# Backup & restore

State lives in **SQLite** (default) or **Postgres** (`DATABASE_URL=postgres://…`).

## What to back up

| Target                                 | Contents                                           |
| -------------------------------------- | -------------------------------------------------- |
| `CODASAURUS_DATA_DIR` / Docker `/data` | `codasaurus.db` (+ WAL/SHM if hot)                 |
| Postgres volume / managed DB           | Full database                                      |
| Secrets store                          | `GITHUB_APP_*`, webhook secret, LLM keys, `OIDC_*` |

Repos, reviews, dismissals, learned rules, and the job queue all sit in that DB.

## SQLite (hot)

```bash
sqlite3 codasaurus.db "PRAGMA wal_checkpoint(TRUNCATE);"
sqlite3 codasaurus.db ".backup 'codasaurus-backup-$(date +%Y%m%d).db'"
```

**Restore:** stop Codasaurus → replace the DB → `rm -f codasaurus.db-wal codasaurus.db-shm` → start.

## Postgres

```bash
pg_dump "$DATABASE_URL" -Fc -f "codasaurus-$(date +%Y%m%d).dump"

# restore
docker compose -f docker-compose.yml -f docker-compose.postgres.yml stop codasaurus
pg_restore -d "$DATABASE_URL" --clean --if-exists codasaurus-YYYYMMDD.dump
docker compose -f docker-compose.yml -f docker-compose.postgres.yml start codasaurus
```

## Multi-replica

- SQLite = single writer.
- Postgres = HA path (`FOR UPDATE SKIP LOCKED` + SHA leases).
- Compose overlay: `docker compose -f docker-compose.yml -f docker-compose.postgres.yml up -d`

## Health

```bash
curl -sf http://localhost:3000/health
curl -sf http://localhost:3000/metrics | head
```

`/health` reports `egress_profile`: `full` · `byok-only` · `offline`.
