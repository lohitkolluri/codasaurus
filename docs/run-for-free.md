# Run Codasaurus on always-free infrastructure

This guide lists **lifetime / always-free** options only — no 30-day trials, no expiring databases, no one-time credits that turn into bills.

Vendors change plans. If a product adds an expiry date, drop it. Prefer options labeled **Always Free** or a permanent **Free** plan with **$0/month and no end date**.

## Policy: what “completely free” means here

| Allowed | Not allowed |
| --- | --- |
| Permanent free plan ($0, no expiry) | “Free for 30/90 days then delete” |
| Soft limits (sleep, pause, storage caps, rate limits) | One-time trial credits (Railway-style) |
| Idle pause you can wake (Neon scale-to-zero, Supabase pause) | 12‑month cloud free tiers (e.g. many RDS offers) |
| Software you run on hardware you already own | “Free compute” that requires a card + auto-charges |

---

## Canonical always-free stack

| Layer | Choice | Why |
| --- | --- | --- |
| **Postgres** | [Neon Free](https://neon.com/pricing) | Permanent free plan, no card, never expires ([FreeTiers](https://www.freetiers.com/directory/neon); [Neon FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier)) |
| **App host** | [Render](https://render.com/pricing) **free web service** | Permanent free *web* plan (sleeps when idle; monthly hour/build caps) — **not** Render free Postgres |
| **LLM** | **Disabled** in the wizard | Tier‑1 detectors need $0 LLM |
| **GitHub** | Free GitHub App | No Codasaurus seats |
| **TLS** | Render HTTPS | Included on free web services |

Optional always-free LLM later: models listed as free on [OpenRouter](https://openrouter.ai) (rate-limited; catalog changes) or **Ollama on your own machine**.

### Explicitly excluded (not lifetime free)

| Thing | Why it’s out |
| --- | --- |
| **Render free Postgres** | Database **expires** after a limited window ([Render free notes](https://agentdeals.dev/vendor/render)) |
| **Railway free** | Trial / usage credit → paid ([free hosting comparisons](https://toolfreebie.com/render-hosting-review)) |
| **AWS RDS “free tier”** | Typically **12 months**, not forever ([Neon FAQ comparison](https://neon.com/faqs/managed-postgres-databases-free-tier)) |
| Any “$X credit then billing” | Not $0 for life |

---

## Deploy (Render web + Neon DB)

### 1. Neon (always-free Postgres)

1. Sign up at [console.neon.tech](https://console.neon.tech/signup) — Free plan, no card ([Neon](https://neon.com/pricing)).
2. Copy a **direct** or **session** connection URI (avoid transaction-only / port **6543**).
3. That string is `DATABASE_URL`. Soft limits: ~0.5 GB storage / project, monthly compute hours, scale-to-zero when idle ([Neon FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier)).

### 2. Render (always-free web service)

1. New **Web Service** → this repo → **Docker** → plan **Free**.
2. Env:

| Key | Value |
| --- | --- |
| `DATABASE_URL` | Neon URI |
| `CODASAURUS_FREE_TIER` | `1` |
| `PUBLIC_URL` | `https://YOUR-SERVICE.onrender.com` |

3. Health check: `/health`
4. Deploy until logs show `Database connected (PostgreSQL)`.

Render free web **sleeps after ~15 minutes** idle and cold-starts in tens of seconds ([Render free guide](https://deploybase.app/blog/render-free-tier-complete-guide-2026)). That is a soft limit, not an expiry. GitHub usually retries webhooks.

### 3. Wizard

Skip LLM → create GitHub App → create admin. Tier‑1 review works at $0.

---

## Always-free Postgres alternatives

Only permanent free plans:

| Provider | Always free? | Notes for Codasaurus |
| --- | --- | --- |
| **Neon Free** | Yes — recommended | Best default; auto pool/TLS tuning in-app |
| **Supabase Free** | Yes (plan does not expire) | ~500 MB; projects **pause after 1 week** idle ([Supabase pricing](https://supabase.com/pricing)) — wake with traffic or a keep-alive; max 2 free projects |
| **Self-host Postgres** on hardware you already have | Yes | `docker compose` Postgres + app; $0 incremental |

Do **not** list Render free Postgres, Railway, or time-boxed cloud DB trials here.

---

## Always-free app hosting alternatives

| Option | Always free? | Fit |
| --- | --- | --- |
| **Render free web** | Yes (with sleep + monthly caps) | Easiest for this Dockerfile |
| **Laptop / home server + tunnel** | Yes | `docker compose up`; expose with a free tunnel if needed |
| **Oracle Cloud Always Free VM** | Yes (Always Free compute/storage) | More setup: run Docker yourself on an Always Free instance ([Oracle Free Tier](https://www.oracle.com/cloud/postgresql) mentions Always Free services + separate 30-day credits — use **Always Free** only, ignore the trial credits) |

Skip Fly/Railway unless you confirm a permanent $0 plan with no card billing.

---

## Always-free LLM

| Option | Lifetime free? | Notes |
| --- | --- | --- |
| **LLM disabled** | Yes | Default for this guide |
| **Ollama on your PC** | Yes | Your electricity; point `CODASAURUS_BASE_URL` at it |
| **OpenRouter free model IDs** | Yes while listed free | Rate limits; not an SLA ([free model lists change](https://costgoat.com/pricing/openrouter-free-models)) |

---

## Local forever-free (no cloud bill)

```bash
docker compose up
```

Postgres + Codasaurus on your machine. Lifetime free. Use a free tunnel only if GitHub must reach you.

---

## Product defaults for this stack

With `CODASAURUS_FREE_TIER=1` or Neon/Supabase/Render URLs:

| Knob | Default |
| --- | --- |
| Pool size | 3 |
| Acquire timeout | 60s (Neon wake) |
| Concurrent reviews | 1 |
| `/health` | HTTP 200 while DB is waking |

Details: [database.md](database.md).

---

## Sources

- [Neon pricing](https://neon.com/pricing) · [Neon free FAQ](https://neon.com/faqs/managed-postgres-databases-free-tier) · [Neon Always Free (FreeTiers)](https://www.freetiers.com/directory/neon)
- [Supabase pricing](https://supabase.com/pricing) (pause after 1 week inactivity)
- [Render pricing](https://render.com/pricing) · [Render free tier guide](https://deploybase.app/blog/render-free-tier-complete-guide-2026) (Mar 2026) · [Render free (AgentDeals)](https://agentdeals.dev/vendor/render) (Jun 2026)
- [Oracle Cloud Free Tier / Always Free](https://www.oracle.com/cloud/postgresql)
- [OpenRouter free models listing](https://costgoat.com/pricing/openrouter-free-models) (Jul 2026)
