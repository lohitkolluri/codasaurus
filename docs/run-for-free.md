# Run Codasaurus on always-free infrastructure

**Lifetime / always-free only** — no 30-day trials, no expiring databases, no one-time credits that turn into bills.

Soft limits (sleep, pause, storage caps) are OK. Time bombs are not. Re-check each vendor’s pricing page; if something gains an expiry date, stop using it.

| Allowed | Not allowed |
| --- | --- |
| Permanent Free / Always Free plan ($0, no end date) | “Free for 30/90 days then delete” |
| Idle sleep/pause you can wake | Trial credits → paid |
| Caps on storage / hours / rate limits | 12‑month cloud free tiers |

---

## Postgres options (ranked)

Best → less ideal for Codasaurus. All are lifetime-free plans (or $0 on hardware you already own).

| Rank | Option | Always free? | Approx. free limit | Fit for Codasaurus | Main catch |
| :---: | --- | --- | --- | --- | --- |
| **1** | [Neon Free](https://neon.com/pricing) | Yes — never expires ([FreeTiers](https://www.freetiers.com/directory/neon)) | ~0.5 GB / project, monthly CU-hours ([Neon FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier)) | **Best default** — URI + TLS; pool auto-tunes | Scale-to-zero cold start |
| **2** | [Supabase Free](https://supabase.com/pricing) | Yes (plan does not expire) | ~500 MB, 2 free projects | Strong alternative; same `DATABASE_URL` flow | Pauses after **1 week** idle ([pricing](https://supabase.com/pricing)) |
| **3** | Self-host Postgres (`docker compose` / your VPS) | Yes ($0 incremental) | Your disk | Full control; no cloud pause | You own backups & uptime |
| **4** | Postgres on [Oracle Always Free](https://www.oracle.com/cloud/postgresql) compute | Yes (Always Free VM/storage) | Always Free VM quotas | Fine if you already use OCI | DIY install; ignore OCI’s separate 30-day *credits* |
| **5** | [Aiven](https://swyftstack.com/blog/free-postgresql-hosting) free Postgres (if still offered) | Confirm “permanent free” on signup | Often larger storage in roundups | OK if plan is still $0 forever | Verify live — free plans change |

**Do not use:** Render free Postgres, Railway credits, AWS RDS 12‑month free — not lifetime free ([Render](https://agentdeals.dev/vendor/render); [Neon FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier)).

Use a **session/direct** URI. Avoid transaction-only poolers (e.g. Supabase port **6543**).

---

## Deployment options (ranked)

Best → less ideal. Host must run the Docker image / binary and accept GitHub webhooks.

| Rank | Option | Always free? | Approx. free limit | Fit for Codasaurus | Main catch |
| :---: | --- | --- | --- | --- | --- |
| **1** | [Render](https://render.com/pricing) free **web service** | Yes (permanent free *web* plan) | ~512 MB RAM, ~750 hrs/mo, sleeps ~15 min ([guide](https://deploybase.app/blog/render-free-tier-complete-guide-2026)) | **Best DX** — Dockerfile + HTTPS | Cold start 30–60s after sleep |
| **2** | Local / home server + free tunnel | Yes | Your machine | Zero cloud bill; great for dogfooding | Must stay online for webhooks |
| **3** | [Oracle Always Free](https://www.oracle.com/cloud/postgresql) VM + Docker | Yes (Always Free compute) | Always Free VM quotas | Always-on-ish if you size it right | More ops than Render |
| **4** | Any always-on box you already pay for | Yes incremental | Your VPS | `docker compose up` in prod | Not “new free cloud,” but $0 extra |
| **5** | Other PaaS free web tiers | Only if permanent $0 | Varies | Possible | Skip if card + auto-bill or trial-only (e.g. Railway credits) |

**Do not use for “forever free”:** Railway trial credits, time-boxed PaaS free DBs bundled as “free hosting.”

---

## Best suggestions (pick a row)

Recommended combinations for people running Codasaurus at $0.

| Suggestion | Postgres | Deploy | LLM | Best for |
| --- | --- | --- | --- | --- |
| **A — Recommended** | Neon (#1) | Render free web (#1) | Off | Most people; least friction |
| **B — Dashboard-friendly DB** | Supabase (#2) | Render free web (#1) | Off | Prefer Supabase UI; add a weekly ping so it doesn’t pause |
| **C — All on your metal** | Compose Postgres (#3) | Same host / tunnel (#2) | Off or Ollama | Max control, no cloud DB |
| **D — Always-on free VM** | Neon (#1) or Compose on VM (#3) | Oracle Always Free VM (#3) | Off | Fewer cold starts than Render sleep |
| **E — Already have a VPS** | Neon (#1) or local Postgres (#3) | Your VPS (#4) | Off / Ollama | Production-ish without new SaaS |

Optional LLM later (still $0): disable is fine; or **Ollama on your PC**; or [OpenRouter](https://openrouter.ai) models currently listed free (rate-limited; list changes).

---

## Quick start (suggestion A)

1. **Neon** — create Free project → copy connection URI → `DATABASE_URL`.
2. **Render** — Web Service → this repo → Docker → Free plan.

| Env | Value |
| --- | --- |
| `DATABASE_URL` | Neon URI |
| `CODASAURUS_FREE_TIER` | `1` |
| `PUBLIC_URL` | `https://YOUR-SERVICE.onrender.com` |

3. Health check `/health` → deploy → wizard: **skip LLM** → GitHub App → admin.
4. `curl -s https://YOUR-SERVICE.onrender.com/health` → HTTP 200.

Set `CODASAURUS_FREE_TIER=1` on any free host. Codasaurus then uses a small pool (3), 60s acquire timeout, and 1 concurrent review. Details: [database.md](database.md).

```bash
# Suggestion C — local forever-free
docker compose up
```

---

## Sources

- [Neon pricing](https://neon.com/pricing) · [Neon free FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier) · [Neon Always Free](https://www.freetiers.com/directory/neon)
- [Supabase pricing](https://supabase.com/pricing)
- [Render pricing](https://render.com/pricing) · [Render free guide](https://deploybase.app/blog/render-free-tier-complete-guide-2026) (Mar 2026) · [Render free (AgentDeals)](https://agentdeals.dev/vendor/render) (Jun 2026)
- [Free PostgreSQL hosting 2026](https://swyftstack.com/blog/free-postgresql-hosting) (Jul 2026)
- [Oracle Cloud Free Tier](https://www.oracle.com/cloud/postgresql)
- [OpenRouter free models listing](https://costgoat.com/pricing/openrouter-free-models) (Jul 2026)
