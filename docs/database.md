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

Unset `DATABASE_URL` falls back to that same local default (**not** on Render — there it is required).

## Render / Neon / Supabase

1. Create a Postgres database.
2. Set `DATABASE_URL` on the Codasaurus service.
3. Redeploy.

**Render-specific**

- Prefer the **Internal Database URL** when the web service and DB are in the **same region** ([Render docs](https://render.com/docs/postgresql-creating-connecting)).
- Codasaurus enables TLS (`sslmode=require`) automatically for non-local hosts.
- Keep pool small on free/shared DBs: default is **5** connections when `RENDER` is set.

**Neon / Supabase**

- Use the **session** pooler or direct URI (not transaction mode / port 6543).
- Include password; SSL is auto-added if missing.

Example:

```text
DATABASE_URL=postgresql://user:pass@host:5432/dbname
# becomes …?sslmode=require for remote hosts
```

### `pool timed out while waiting for an open connection`

Usually means the app could not get a live DB connection in time:

| Check | Fix |
| --- | --- |
| Missing / wrong `DATABASE_URL` | Set Internal URL from Render DB → Info |
| External URL + no TLS | Redeploy latest Codasaurus (TLS built-in) or add `?sslmode=require` |
| Wrong region / unreachable host | Same-region Internal URL |
| Connection cap exhausted | Lower `CODASAURUS_DB_MAX_CONNECTIONS` (e.g. `3`) |
| Neon paused / cold start | Wait and retry; raise `CODASAURUS_DB_ACQUIRE_TIMEOUT_SECS` (default `30`) |

## Pool tuning

| Variable | Default | Notes |
| --- | --- | --- |
| `CODASAURUS_DB_MAX_CONNECTIONS` | `16` local / `5` on Render | Clamped 2–64; stay under Postgres `max_connections` |
| `CODASAURUS_DB_ACQUIRE_TIMEOUT_SECS` | `30` | Wait for a free connection / slow wake |
| idle timeout | 10m | Recycle idle clients |
| max lifetime | 30m | Align with PG / proxy lifetimes |

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
