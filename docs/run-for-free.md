# Run Codasaurus for $0 (forever)

<p>
  <img src="https://img.shields.io/badge/%240-always%20free-2ea44f" alt="Always free">
  <img src="https://img.shields.io/badge/db-Aiven%20%7C%20Neon%20%7C%20Supabase-4169E1" alt="Postgres">
  <img src="https://img.shields.io/badge/host-Render%20free%20web-46E3B7" alt="Render">
  <a href="README.md"><img src="https://img.shields.io/badge/docs-index-111827" alt="Docs index"></a>
</p>

No credit-card landmines. No “free for 30 days then we delete your database.” Soft limits (sleep, pause, storage caps) are fine. **Time bombs are not.**


Vendors rename plans every quarter. If something suddenly has an expiry date, drop it.

| Green light                                    | Red light                         |
| ---------------------------------------------- | --------------------------------- |
| Permanent Free / Always Free ($0, no end date) | “Free for 30/90 days then delete” |
| Idle sleep you can wake                        | Trial credits → surprise invoice  |
| Caps on storage / hours / rate limits          | The classic “12‑month free tier”  |

---

## Postgres (ranked)

Best pick first. All of these are lifetime-free (or $0 on hardware you already own).

|   #   | Pick                                                                            | The vibe                                                                                   | Free ceiling                                                                                                                                                              | Catch                                                                                                                                |
| :---: | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| **1** | [Aiven Free](https://aiven.io/postgresql) Postgres                              | **Always-on** small node. No Neon-style nap. Codasaurus auto-detects `*.aivencloud.com`.  | 1 CPU / 1 GB RAM / 1 GB disk, `max_connections=20`, no SLA ([free tier](https://github.com/aiven/aiven-docs/blob/main/docs/products/postgresql/concepts/pg-free-tier.md)) | One free PG per org; paste **Service URI** with `sslmode=require` ([connect](https://aiven.io/docs/products/postgresql/get-started)) |
| **2** | [Neon Free](https://neon.com/pricing)                                           | Paste a URI and go. Never expires ([FreeTiers](https://www.freetiers.com/directory/neon)). | ~0.5 GB / project, monthly CU-hours ([FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier))                                                                   | Scale-to-zero cold starts (can look like `pool timed out`)                                                                           |
| **3** | [Supabase Free](https://supabase.com/pricing)                                   | Same `DATABASE_URL` story, prettier dashboard                                              | ~500 MB, 2 free projects                                                                                                                                                  | Ghosts you after **1 week** idle. Poke it weekly                                                                                    |
| **4** | Self-host (`docker compose` / your box)                                         | Full control, zero cloud drama                                                             | Your disk                                                                                                                                                                 | You are ops now                                                                                                                      |
| **5** | Postgres on an [Oracle Always Free](https://www.oracle.com/cloud/postgresql) VM | Fine if you already live in OCI                                                            | Always Free quotas                                                                                                                                                        | DIY install. Use **Always Free**, ignore the 30-day credit bait                                                                     |

**Hard no:** Render free Postgres, Railway credits, AWS RDS “free for a year.” Those are rental cars with a return date ([Render](https://agentdeals.dev/vendor/render); [Neon FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier)).

Tip: use the **direct Service URI**. Avoid transaction-only poolers (e.g. Supabase port **6543**). Aiven free has no connection pooling anyway. Just the service URI.

---

## Deploy (ranked)

Where the binary lives and GitHub can yell at it.

|   #   | Pick                                                                      | The vibe                                 | Free ceiling                                                                                                         | Catch                                                         |
| :---: | ------------------------------------------------------------------------- | ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| **1** | [Render](https://render.com/pricing) free **web service**                 | Easiest path for this Dockerfile + HTTPS | ~512 MB RAM, ~750 hrs/mo, sleeps ~15 min ([guide](https://deploybase.app/blog/render-free-tier-complete-guide-2026)) | First request after a nap: 30–60s. GitHub usually retries.    |
| **2** | Laptop / home lab + free tunnel                                           | Dogfood on hardware you already own      | Your uptime                                                                                                          | Laptop closed = webhooks ghosted                              |
| **3** | [Oracle Always Free](https://www.oracle.com/cloud/postgresql) VM + Docker | Closer to “always on” without a bill     | Always Free VM quotas                                                                                                | More buttons than Render                                      |
| **4** | A VPS you already pay for                                                 | Not new free cloud, just $0 _extra_     | Whatever you rented                                                                                                  | Still money somewhere, just not _new_ money                   |
| **5** | Other PaaS free web tiers                                                 | Maybe                                    | Varies                                                                                                               | Bail if it’s “card + trial credits” (looking at you, Railway) |

---

## Best stacks (pick your fighter)

Don’t overthink it. One row, ship it.

| Stack                     | Postgres                             | Deploy                     | LLM           | Who it’s for                                                   |
| ------------------------- | ------------------------------------ | -------------------------- | ------------- | -------------------------------------------------------------- |
| **A: Recommended**       | Aiven Free (#1)                      | Render web (#1)            | Off           | Best $0 cloud combo. DB stays warm while Render sleeps        |
| **B: Neon path**         | Neon (#2)                            | Render web (#1)            | Off           | Fine; wake Neon in the console if boot times out               |
| **C: Pretty DB UI**      | Supabase (#3)                        | Render web (#1)            | Off           | Like Supabase’s console; weekly ping so it doesn’t nap forever |
| **D: Metal mode**        | Compose Postgres (#4)                | Same machine / tunnel (#2) | Off or Ollama | Control freaks and air-gap enjoyers                            |
| **E: Always-on free VM** | Aiven (#1) or Compose on the VM (#4) | Oracle Always Free (#3)    | Off           | Hate Render cold starts                                        |

LLM can stay off forever. Tier-1 detectors don’t need one. Later, if you want brains on a budget: **Ollama on your PC**, or whatever [OpenRouter](https://openrouter.ai) currently lists as free (rate-limited; catalog moves).

---

## 10-minute path (stack A: Aiven + Render)

1. [Aiven Console](https://console.aiven.io/) → create **PostgreSQL** on the **Free** plan → **Overview → Quick connect** → copy the Service URI (`…aivencloud.com:PORT/defaultdb?sslmode=require`). That’s `DATABASE_URL`.
2. [Render](https://render.com) → New **Web Service** → this repo → **Docker** → plan **Free**.

| Env                    | Value                               |
| ---------------------- | ----------------------------------- |
| `DATABASE_URL`         | Aiven Service URI                   |
| `CODASAURUS_FREE_TIER` | `1`                                 |
| `PUBLIC_URL`           | `https://YOUR-SERVICE.onrender.com` |

3. Health check path: `/health`. Deploy until logs say `Database connected (PostgreSQL)` and `free_tier=true` (Aiven host is auto-detected).
4. Wizard: **skip LLM** → GitHub App → admin. Then:

```bash
curl -s https://YOUR-SERVICE.onrender.com/health
# want: HTTP 200
```

### If you see `pool timed out while waiting for an open connection`

| Likely cause                   | Fix                                                                                   |
| ------------------------------ | ------------------------------------------------------------------------------------- |
| Wrong / missing `DATABASE_URL` | Paste Aiven **Service URI** (host like `….a.aivencloud.com`, non‑5432 port is normal) |
| Missing TLS                    | URI should include `sslmode=require`. Codasaurus adds it for remote hosts if missing |
| Neon asleep _(if on Neon)_     | Open the Neon console once, redeploy                                                  |
| Transaction pooler             | Avoid Supabase port **6543**                                                          |
| Render free Postgres           | Don’t. It expires                                                                    |
| Stale deploy                   | Manual Deploy of latest `main`                                                        |

`CODASAURUS_FREE_TIER=1` (or an Aiven/Neon/Supabase URL) turns on free-host manners: pool of 3, longer timeouts, one review at a time, well under Aiven free’s `max_connections=20`. More in [database.md](database.md).

```bash
# Stack D: zero cloud, zero excuses
docker compose up
```

---

## Sources

- [Aiven PostgreSQL](https://aiven.io/postgresql) · [Free tier limits](https://github.com/aiven/aiven-docs/blob/main/docs/products/postgresql/concepts/pg-free-tier.md) · [Get started / connect](https://aiven.io/docs/products/postgresql/get-started) · [TLS notes](https://aiven.io/docs/platform/concepts/tls-ssl-certificates)
- [Neon pricing](https://neon.com/pricing) · [Neon free FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier)
- [Supabase pricing](https://supabase.com/pricing)
- [Render pricing](https://render.com/pricing) · [Render free guide](https://deploybase.app/blog/render-free-tier-complete-guide-2026) · [Render free notes](https://agentdeals.dev/vendor/render)
- [Oracle Cloud Free Tier](https://www.oracle.com/cloud/postgresql)
- [OpenRouter free models](https://costgoat.com/pricing/openrouter-free-models)
