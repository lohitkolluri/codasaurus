# Database — PostgreSQL

Codasaurus uses **one PostgreSQL database** for all durable state: repos, reviews, findings, the review job queue, sessions, learning, webhooks, and `agent_events`.

There is no SQLite mode and no Redis requirement.

## Quick start

```bash
docker compose up
```

Compose starts Postgres 16 and sets:

```text
DATABASE_URL=postgres://codasaurus:…@postgres:5432/codasaurus
```

From source:

```bash
export DATABASE_URL="postgres://codasaurus:codasaurus@127.0.0.1:5432/codasaurus"
codasaurus serve --port 3000
```

Unset `DATABASE_URL` falls back to that same local default.

## Pool tuning

| Variable                        | Default             | Notes                                                       |
| ------------------------------- | ------------------- | ----------------------------------------------------------- |
| `CODASAURUS_DB_MAX_CONNECTIONS` | `16` (clamped 2–64) | API + review workers; keep below Postgres `max_connections` |
| acquire timeout                 | 5s                  | Fail fast under pool pressure                               |
| idle timeout                    | 10m                 | Recycle idle clients                                        |
| max lifetime                    | 30m                 | Align with PG / proxy connection lifetimes                  |

## Schema highlights

- Timestamps are `TIMESTAMPTZ` (not `TEXT`)
- Job claim uses `FOR UPDATE SKIP LOCKED` with partial indexes on `pending` / `running`
- Findings insert via `UNNEST` (one round-trip per review)
- Sessions expire via `expires_at > NOW()`; cleanup is periodic (not on every auth)

Migrations run automatically on boot (`schema_version` through **v11**).

## Multi-replica

Point every Codasaurus replica at the **same** `DATABASE_URL`. The queue and SHA leases are multi-writer safe.

## Backup

See [operations-backup-restore.md](operations-backup-restore.md).
