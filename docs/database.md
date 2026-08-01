# Database (PostgreSQL)

<p>
  <img src="https://img.shields.io/badge/db-PostgreSQL-4169E1?logo=postgresql&logoColor=white" alt="PostgreSQL">
  <img src="https://img.shields.io/badge/sqlite-gone-lightgrey" alt="No SQLite">
  <a href="run-for-free.md"><img src="https://img.shields.io/badge/%240-free%20hosts-2ea44f" alt="Free hosts"></a>
  <a href="README.md"><img src="https://img.shields.io/badge/docs-index-111827" alt="Docs index"></a>
</p>

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

Unset `DATABASE_URL` falls back to that same local default (**not** on Render; there it is required).

**$0 forever (no trials):** [run-for-free.md](run-for-free.md). Render free web + Aiven Free Postgres (or Neon).

## Cloud Postgres (Aiven / Neon / Supabase)

1. Create a Postgres database on an always-free plan (or self-host).
2. Set `DATABASE_URL` on the Codasaurus service.
3. Redeploy.

**Tips**

- Codasaurus enables TLS (`sslmode=require`) automatically for non-local hosts.
- Free / shared DBs get a small pool by default (**3** when `RENDER`, `CODASAURUS_FREE_TIER=1`, or the URL looks like Aiven / Neon / Supabase).
- **Aiven:** paste the Service URI from Quick connect (`*.aivencloud.com`, non-5432 port is normal).
- **Neon / Supabase:** use the **session** pooler or direct URI (not transaction mode / port 6543).
- Skip **Render free Postgres** (it expires). Keep Render for the *web* service only.

Example:

```text
DATABASE_URL=postgresql://user:pass@host:5432/dbname
# becomes …?sslmode=require for remote hosts
```

### `pool timed out while waiting for an open connection`

The app could not open a live Postgres connection before the timeout. On free stacks this is almost always **URL / wake**, not “need a bigger pool.”

| Check | Fix |
| --- | --- |
| Missing / wrong `DATABASE_URL` | Aiven Service URI or Neon/Supabase session/direct URI ([run-for-free.md](run-for-free.md)) |
| Neon compute asleep | Open the Neon console once, then redeploy |
| TLS | Latest Codasaurus adds `sslmode=require` for remote hosts automatically |
| Transaction pooler (`:6543`) | Use session/direct instead |
| Render free Postgres | Expires. Switch to Aiven or Neon |
| Still stuck | Set `CODASAURUS_DB_ACQUIRE_TIMEOUT_SECS=90` and `CODASAURUS_FREE_TIER=1` |

Logs should show `Connecting to PostgreSQL at host:port/db` (password never printed).

## Pool tuning

| Variable | Default | Notes |
| --- | --- | --- |
| `CODASAURUS_DB_MAX_CONNECTIONS` | `16` local / `3` free-tier | Clamped 2–64 |
| `CODASAURUS_DB_ACQUIRE_TIMEOUT_SECS` | `30` local / `60` free-tier | Neon wake + pool wait |
| idle timeout | 10m | Recycle idle clients |
| max lifetime | 30m | Align with PG / proxy lifetimes |

## Schema highlights

- Timestamps are `TIMESTAMPTZ` (not `TEXT`)
- Job claim uses `FOR UPDATE SKIP LOCKED` with partial indexes on `pending` / `running`
- Findings insert via `UNNEST` (one round-trip per review)
- Sessions expire via `expires_at > NOW()`; cleanup is periodic (not on every auth)

Migrations run automatically on boot (`schema_version` through **v15**).

## Multi-replica

Point every Codasaurus replica at the **same** `DATABASE_URL`. The queue and SHA leases are multi-writer safe.

## Backup

See [operations-backup-restore.md](operations-backup-restore.md).
