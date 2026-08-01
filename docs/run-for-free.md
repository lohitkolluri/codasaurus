# Run Codasaurus for $0 (forever)

No credit-card landmines. No “free for 30 days then we delete your database.” Soft limits (sleep, pause, storage caps) are fine. **Time bombs are not.**

Vendors rename plans every quarter. If something suddenly has an expiry date, drop it.

| Green light                                    | Red light                         |
| ---------------------------------------------- | --------------------------------- |
| Permanent Free / Always Free ($0, no end date) | “Free for 30/90 days then delete” |
| Idle sleep you can wake                        | Trial credits → surprise invoice  |
| Caps on storage / hours / rate limits          | The classic “12‑month free tier”  |

---

## Postgres — ranked

Best pick first. All of these are lifetime-free (or $0 on hardware you already own).

|   #   | Pick                                                                                            | The vibe                                                                                            | Free ceiling                                                                                            | Catch                                                            |
| :---: | ----------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| **1** | [Neon Free](https://neon.com/pricing)                                                           | Default. Paste a URI and go. Never expires ([FreeTiers](https://www.freetiers.com/directory/neon)). | ~0.5 GB / project, monthly CU-hours ([FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier)) | Naps when idle — cold start is real                              |
| **2** | [Supabase Free](https://supabase.com/pricing)                                                   | Same `DATABASE_URL` story, prettier dashboard                                                       | ~500 MB, 2 free projects                                                                                | Ghosts you after **1 week** idle — poke it weekly                |
| **3** | Self-host (`docker compose` / your box)                                                         | Full control, zero cloud drama                                                                      | Your disk                                                                                               | You are ops now                                                  |
| **4** | Postgres on an [Oracle Always Free](https://www.oracle.com/cloud/postgresql) VM                 | Fine if you already live in OCI                                                                     | Always Free quotas                                                                                      | DIY install — use **Always Free**, ignore the 30-day credit bait |
| **5** | [Aiven](https://swyftstack.com/blog/free-postgresql-hosting) free Postgres _(if still offered)_ | Sometimes roomier on paper                                                                          | Check the live plan                                                                                     | Free plans vanish — confirm “$0 forever” on signup               |

**Hard no:** Render free Postgres, Railway credits, AWS RDS “free for a year.” Those are rental cars with a return date ([Render](https://agentdeals.dev/vendor/render); [Neon FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier)).

Tip: use a **session/direct** URI. Transaction poolers (Supabase port **6543**) will make sqlx sad.

---

## Deploy — ranked

Where the binary lives and GitHub can yell at it.

|   #   | Pick                                                                      | The vibe                                 | Free ceiling                                                                                                         | Catch                                                         |
| :---: | ------------------------------------------------------------------------- | ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| **1** | [Render](https://render.com/pricing) free **web service**                 | Easiest path for this Dockerfile + HTTPS | ~512 MB RAM, ~750 hrs/mo, sleeps ~15 min ([guide](https://deploybase.app/blog/render-free-tier-complete-guide-2026)) | First request after a nap: 30–60s. GitHub usually retries.    |
| **2** | Laptop / home lab + free tunnel                                           | Dogfood on hardware you already own      | Your uptime                                                                                                          | Laptop closed = webhooks ghosted                              |
| **3** | [Oracle Always Free](https://www.oracle.com/cloud/postgresql) VM + Docker | Closer to “always on” without a bill     | Always Free VM quotas                                                                                                | More buttons than Render                                      |
| **4** | A VPS you already pay for                                                 | Not new free cloud — just $0 _extra_     | Whatever you rented                                                                                                  | Still money somewhere, just not _new_ money                   |
| **5** | Other PaaS free web tiers                                                 | Maybe                                    | Varies                                                                                                               | Bail if it’s “card + trial credits” (looking at you, Railway) |

---

## Best stacks — pick your fighter

Don’t overthink it. One row, ship it.

| Stack                        | Postgres                            | Deploy                     | LLM           | Who it’s for                                                         |
| ---------------------------- | ----------------------------------- | -------------------------- | ------------- | -------------------------------------------------------------------- |
| **A — Just ship it**         | Neon (#1)                           | Render web (#1)            | Off           | You. Probably.                                                       |
| **B — Pretty DB UI**         | Supabase (#2)                       | Render web (#1)            | Off           | Like Supabase’s console; set a weekly ping so it doesn’t nap forever |
| **C — Metal mode**           | Compose Postgres (#3)               | Same machine / tunnel (#2) | Off or Ollama | Control freaks and air-gap enjoyers                                  |
| **D — Always-on free VM**    | Neon (#1) or Compose on the VM (#3) | Oracle Always Free (#3)    | Off           | Hate Render cold starts                                              |
| **E — I already have a box** | Neon (#1) or local Postgres (#3)    | Your VPS (#4)              | Off / Ollama  | Production-ish without hunting new SaaS                              |

LLM can stay off forever — Tier‑1 detectors don’t need one. Later, if you want brains on a budget: **Ollama on your PC**, or whatever [OpenRouter](https://openrouter.ai) currently lists as free (rate-limited; catalog moves).

---

## 10-minute path (stack A)

1. [Neon](https://console.neon.tech/signup) → Free project → copy the connection URI → that’s `DATABASE_URL`.
2. [Render](https://render.com) → New **Web Service** → this repo → **Docker** → plan **Free**.

| Env                    | Value                               |
| ---------------------- | ----------------------------------- |
| `DATABASE_URL`         | Neon URI                            |
| `CODASAURUS_FREE_TIER` | `1`                                 |
| `PUBLIC_URL`           | `https://YOUR-SERVICE.onrender.com` |

3. Health check path: `/health`. Deploy until logs say `Database connected (PostgreSQL)` — **not** `(SQLite)` (that means an old image).
4. Wizard: **skip LLM** → GitHub App → admin. Then:

```bash
curl -s https://YOUR-SERVICE.onrender.com/health
# want: HTTP 200
```

### If you see `pool timed out while waiting for an open connection`

| Likely cause | Fix |
| --- | --- |
| Wrong / missing `DATABASE_URL` | Paste Neon **direct** or **session** URI (host like `ep-….neon.tech`) |
| Neon asleep | Open [console.neon.tech](https://console.neon.tech), click the project once, redeploy |
| Transaction pooler | Avoid Supabase port **6543** — use session/direct |
| Render free Postgres | Don’t — it expires. Use Neon |
| Stale deploy | Manual Deploy of latest `main` (must log `Connecting to PostgreSQL at …`) |

`CODASAURUS_FREE_TIER=1` (or a Neon/Supabase URL) turns on free-host manners: pool of 3, longer timeouts, one review at a time. More in [database.md](database.md).

```bash
# Stack C — zero cloud, zero excuses
docker compose up
```

---

## Sources

- [Neon pricing](https://neon.com/pricing) · [Neon free FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier) · [Neon Always Free](https://www.freetiers.com/directory/neon)
- [Supabase pricing](https://supabase.com/pricing)
- [Render pricing](https://render.com/pricing) · [Render free guide](https://deploybase.app/blog/render-free-tier-complete-guide-2026) · [Render free notes](https://agentdeals.dev/vendor/render)
- [Free PostgreSQL hosting 2026](https://swyftstack.com/blog/free-postgresql-hosting)
- [Oracle Cloud Free Tier](https://www.oracle.com/cloud/postgresql)
- [OpenRouter free models](https://costgoat.com/pricing/openrouter-free-models)
