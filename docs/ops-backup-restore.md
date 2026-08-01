# Operations: Backup & Restore

Codasaurus persists everything in one SQLite database (`DATABASE_URL`, default
`sqlite://$CODASAURUS_DATA_DIR/codasaurus.db`).

## What to back up

| Path / var | Contents |
| --- | --- |
| `CODASAURUS_DATA_DIR` (default OS data dir) | `codasaurus.db` (+ `-wal` / `-shm` if hot) |
| Docker volume `/data` | Same when using the official image |
| Env / secrets store | `GITHUB_APP_*`, `OPENROUTER_API_KEY`, webhook secret |

Dashboard settings, repos, reviews, dismissals, learned rules, and `review_jobs`
all live in that SQLite file.

## Hot backup (recommended)

While the server is running (WAL mode):

```bash
# Inside the data directory
sqlite3 codasaurus.db "PRAGMA wal_checkpoint(TRUNCATE);"
sqlite3 codasaurus.db ".backup 'codasaurus-backup-$(date +%Y%m%d).db'"
```

Or copy after checkpoint:

```bash
sqlite3 "$CODASAURUS_DATA_DIR/codasaurus.db" "PRAGMA wal_checkpoint(FULL);"
cp "$CODASAURUS_DATA_DIR/codasaurus.db" "/backups/codasaurus-$(date +%Y%m%d%H%M).db"
```

## Cold backup

```bash
# Stop the process / compose stack first
docker compose stop
cp ./data/codasaurus.db ./backups/codasaurus.db
docker compose start
```

## Restore

1. Stop Codasaurus.
2. Replace the DB file with the backup (keep permissions owned by the runtime user).
3. Remove stale WAL/SHM if present: `rm -f codasaurus.db-wal codasaurus.db-shm`
4. Start Codasaurus. Migrations are idempotent and will no-op on a current schema.

```bash
docker compose stop
cp ./backups/codasaurus-YYYYMMDD.db ./data/codasaurus.db
rm -f ./data/codasaurus.db-wal ./data/codasaurus.db-shm
docker compose start
```

## Multi-replica notes

- SQLite is single-writer. Run **one** `codasaurus serve` writer, or put the DB on
  networked block storage only if you accept SQLite locking limits.
- SHA leases use `lease_owner` (`HOSTNAME`/`CODASAURUS_INSTANCE_ID` + pid) so a
  second replica will not steal an active in-progress review until the lease is
  stale (~10 minutes).
- For true HA, plan a Postgres dual-backend (not enabled as the runtime DB yet).

## Health checks

```bash
curl -sf http://localhost:3000/health
curl -sf http://localhost:3000/metrics | head
```

`codasaurus_review_latency_ms`, `codasaurus_github_429_total`, and
`codasaurus_queue_*` counters are useful for capacity alerts.
