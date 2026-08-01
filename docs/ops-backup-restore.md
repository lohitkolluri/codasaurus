# Operations: Backup & Restore

Codasaurus persists state in SQLite (default) or PostgreSQL when
`DATABASE_URL=postgres://…` / `postgresql://…`.

## What to back up

| Path / var                                  | Contents                                                                      |
| ------------------------------------------- | ----------------------------------------------------------------------------- |
| `CODASAURUS_DATA_DIR` (default OS data dir) | `codasaurus.db` (+ `-wal` / `-shm` if hot) — SQLite mode                      |
| Docker volume `/data`                       | Same when using the official image                                            |
| Postgres volume / managed DB                | Full DB when using `docker-compose.postgres.yml`                              |
| Env / secrets store                         | `GITHUB_APP_*`, `OPENROUTER_API_KEY`, webhook secret, `OIDC_*`, ticket tokens |

Dashboard settings, repos, reviews, dismissals, learned rules, and `review_jobs`
all live in that database.

## Hot backup — SQLite (recommended)

While the server is running (WAL mode):

```bash
sqlite3 codasaurus.db "PRAGMA wal_checkpoint(TRUNCATE);"
sqlite3 codasaurus.db ".backup 'codasaurus-backup-$(date +%Y%m%d).db'"
```

## Hot backup — Postgres

```bash
pg_dump "$DATABASE_URL" -Fc -f "codasaurus-$(date +%Y%m%d).dump"
docker compose -f docker-compose.yml -f docker-compose.postgres.yml exec postgres \
  pg_dump -U codasaurus -Fc codasaurus > "codasaurus-$(date +%Y%m%d).dump"
```

## Restore — SQLite

1. Stop Codasaurus.
2. Replace the DB file; remove stale WAL/SHM: `rm -f codasaurus.db-wal codasaurus.db-shm`
3. Start Codasaurus.

## Restore — Postgres

```bash
docker compose -f docker-compose.yml -f docker-compose.postgres.yml stop codasaurus
pg_restore -d "$DATABASE_URL" --clean --if-exists codasaurus-YYYYMMDD.dump
docker compose -f docker-compose.yml -f docker-compose.postgres.yml start codasaurus
```

## Multi-replica notes

- **SQLite** is single-writer — one `codasaurus serve` writer.
- **Postgres** is the production HA path; workers claim jobs with `FOR UPDATE SKIP LOCKED`.
- SHA leases use `lease_owner` (`HOSTNAME` / `CODASAURUS_INSTANCE_ID` + pid).
- Overlay: `docker compose -f docker-compose.yml -f docker-compose.postgres.yml up -d`

## Success metric targets (measurement, not hard fail)

| Signal                                          | Target                                    |
| ----------------------------------------------- | ----------------------------------------- |
| `codasaurus_review_latency_ms{quantile="0.95"}` | under 60000 ms for ≤200-file PRs          |
| `codasaurus_fp_proxy_ratio`                     | under 0.05 (dismissals / Tier-1 findings) |
| `codasaurus_queue_depth`                        | alert if pending grows without completing |

## Health checks

```bash
curl -sf http://localhost:3000/health
curl -sf http://localhost:3000/metrics | head
```
