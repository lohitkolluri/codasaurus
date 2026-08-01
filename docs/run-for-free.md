# Run Codasaurus completely free

Codasaurus is designed so a solo operator can run a real GitHub App **without paying** for seats, a SaaS reviewer, or (with the right free tiers) hosting/DB/LLM.

Limits change. Re-check each provider’s pricing page before you depend on them.

## Recommended free stack (2026)

| Layer | Pick | Why it fits Codasaurus |
| --- | --- | --- |
| **App host** | [Render](https://render.com/pricing) free web service | Git deploy, HTTPS, 750 hrs/mo, no card ([Render free guide](https://deploybase.app/blog/render-free-tier-complete-guide-2026)) |
| **Postgres** | [Neon](https://neon.com/pricing) Free | Always-free, no 30-day DB expiry; scale-to-zero ([Neon free FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier)) |
| **LLM** | Skip in wizard **or** [OpenRouter](https://openrouter.ai) free models **or** local Ollama | Tier‑1 detectors work with LLM disabled |
| **GitHub** | Free GitHub App + public/private repos you already have | No Codasaurus seat tax |
| **DNS / TLS** | Render custom domain + Let’s Encrypt | Free SSL on free web services |

**Do not** use Render’s free Postgres as your long-term DB — it **expires** (reports vary 30–90 days) ([Render free tier notes](https://agentdeals.dev/vendor/render); [Tool Freebie review](https://toolfreebie.com/render-hosting-review)). Pair **Render (app) + Neon (DB)** instead.

### Why this combo

- Codasaurus is a **long-running webhook receiver**. Render free will **sleep after ~15 minutes** idle and cold-start in **30–60s** ([AgentDeals / Render](https://agentdeals.dev/vendor/render)). GitHub retries webhooks; first review after idle may be slow — that’s the free-tier tradeoff.
- Neon **scale-to-zero** after idle is fine if acquire timeout is generous (Codasaurus defaults to **60s** on Neon/Supabase/Render hosts).
- Supabase Free also works (~500 MB) but may **pause after ~1 week** idle ([Neon comparison](https://neon.com/faqs/managed-postgres-databases-free-tier)).

---

## 30-minute free deploy

### 1. Neon Postgres (free)

1. Create a project at [console.neon.tech](https://console.neon.tech/signup) (no card).
2. Copy the connection string (**pooled session** or direct — avoid transaction-only port **6543**).
3. Keep it handy as `DATABASE_URL`.

### 2. Render web service (free)

1. New **Web Service** → connect this GitHub repo.
2. Runtime: **Docker** (uses the repo `Dockerfile`).
3. Instance: **Free**.
4. Environment:

| Key | Value |
| --- | --- |
| `DATABASE_URL` | Neon URI (SSL is auto-added for remote hosts) |
| `CODASAURUS_FREE_TIER` | `1` |
| `PUBLIC_URL` | `https://YOUR-SERVICE.onrender.com` |
| `PORT` | Leave unset (Render injects it) |

Optional later: `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY_B64`, `GITHUB_WEBHOOK_SECRET`, `OPENROUTER_API_KEY`.

5. Health check path: `/health` (liveness — won’t restart-loop while Neon wakes).
6. Deploy. Logs should show `Database connected (PostgreSQL)`.

### 3. Onboarding

1. Open `PUBLIC_URL` → wizard.
2. Confirm database → **Skip LLM** (or add a free OpenRouter model later).
3. Create GitHub App → install on a test repo → create admin user.

### 4. Verify

```bash
curl -s https://YOUR-SERVICE.onrender.com/health
# expect HTTP 200; "db": true once Neon is warm
```

Open a PR; wait through cold start if the service slept.

---

## Free Postgres options (compared)

| Provider | Always free? | Storage (approx.) | Catch for Codasaurus |
| --- | --- | --- | --- |
| **Neon Free** | Yes | ~0.5 GB / project | Cold compute after idle ([Neon](https://neon.com/faqs/managed-postgres-databases-free-tier)) |
| **Supabase Free** | Yes | ~500 MB | Pause after ~7 days idle; 2 free projects ([Neon FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier)) |
| **Aiven Free** | Yes (check current plan) | Up to ~5 GB in some roundups | Single-node; confirm on Aiven’s site ([free PG roundup](https://swyftstack.com/blog/free-postgresql-hosting)) |
| **Render free PG** | Temporary | Small | **Expires** — use only for demos ([Render](https://agentdeals.dev/vendor/render)) |

Codasaurus auto-tunes when it sees Neon / Supabase / Render / Aiven hosts: **3** pool connections, **60s** acquire timeout, TLS (`sslmode=require`). Override with `CODASAURUS_DB_*` if needed. Details: [database.md](database.md).

---

## Free app hosting options

| Provider | Free shape | Fit |
| --- | --- | --- |
| **Render** free web | 512 MB RAM, sleeps 15 min, 750 hrs/mo | **Best DX** for this repo’s Dockerfile |
| **Fly.io** | Often needs card; small free allowance historically | Always-on-ish VMs if you qualify — more ops |
| **Railway** | Trial / credit, then paid | Not “forever free” |
| **Your own VPS** you already pay for | `docker compose up` | Truly uncapped “free incremental” cost |

---

## Free LLM options

Tier‑1 detectors (secrets, phantom deps, OSV, IaC, …) run **without any LLM**.

| Option | Cost | Notes |
| --- | --- | --- |
| **Disabled** in wizard | $0 | Recommended for free deploys |
| **OpenRouter free models** | $0 (rate-limited) | See OpenRouter’s free catalog ([costgoat listing](https://costgoat.com/pricing/openrouter-free-models)); not for hard prod SLA |
| **Ollama** on a machine you own | $0 | Point `CODASAURUS_BASE_URL` at it; keep Codasaurus on Render |

---

## Free-tier product defaults

When `CODASAURUS_FREE_TIER=1` or host is Render / free cloud Postgres URL:

| Knob | Free default |
| --- | --- |
| DB pool size | 3 |
| DB acquire timeout | 60s |
| Concurrent reviews | 1 |
| `/health` | 200 even if DB is briefly waking (`"status":"degraded"`) |

Set `CODASAURUS_FREE_TIER=1` explicitly on any free host.

---

## Gotchas (read these)

1. **Cold starts** — first webhook after sleep can take a minute; GitHub usually retries.
2. **Ephemeral disk on Render** — don’t store state on local disk; Postgres is the source of truth.
3. **Build minutes** — Render free has limited build minutes/month; prefer Docker layer cache / fewer force rebuilds.
4. **Don’t point at localhost Postgres** from Render — that caused many “pool timed out” deploys.
5. **Transaction poolers (Supabase :6543)** — avoid; use session/direct for `SKIP LOCKED`.

---

## Local free (zero cloud)

```bash
docker compose up
```

Postgres + Codasaurus on your laptop. Still free; not reachable by GitHub until you tunnel (`cloudflared`, etc.).

---

## When free stops being enough

Move to paid always-on compute and/or a non-sleeping DB when:

- Review latency after idle is unacceptable
- You exceed Neon storage / compute-hours
- Multiple replicas need a larger pool

Migration is a `DATABASE_URL` swap + `pg_dump` / restore — see [operations-backup-restore.md](operations-backup-restore.md).

---

## Sources

- [Render pricing](https://render.com/pricing)
- [Render free tier 2026 guide](https://deploybase.app/blog/render-free-tier-complete-guide-2026) (Mar 2026)
- [Render free tier (AgentDeals)](https://agentdeals.dev/vendor/render) (Jun 2026)
- [Render free hosting review](https://toolfreebie.com/render-hosting-review) (Jul 2026)
- [Neon free plan FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier)
- [Neon pricing](https://neon.com/pricing) · [Neon free tier (AgentDeals)](https://agentdeals.dev/vendor/neon) (Jun 2026)
- [Free PostgreSQL hosting 2026](https://swyftstack.com/blog/free-postgresql-hosting) (Jul 2026)
- [OpenRouter free models listing](https://costgoat.com/pricing/openrouter-free-models) (Jul 2026)
