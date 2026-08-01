<p align="center">
  <img src="assets/logo.svg" alt="Codasaurus" width="96">
</p>

<h1 align="center">Codasaurus</h1>

<p align="center">
  <b>Self-hosted GitHub App that reviews PRs like a senior who actually reads the diff.</b>
</p>

<p align="center">
  Deterministic Tier‑1 detectors · optional BYOK LLM · zero seat tax<br>
  <code>@codasaurus</code> on any PR · Svelte dashboard · PostgreSQL
</p>

<p align="center">
  <a href="https://github.com/lohitkolluri/codasaurus/actions/workflows/ci.yml"><img src="https://github.com/lohitkolluri/codasaurus/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/rust-1.88+-blue" alt="Rust">
  <img src="https://img.shields.io/github/license/lohitkolluri/codasaurus" alt="License">
</p>

---

Agents ship volume. Models invent packages, paste last year's APIs, and leave secrets in the hunk. SaaS reviewers charge per seat and still miss the boring breaks.

**Codasaurus** is one Rust binary you run yourself. On every PR it proves what it can (registry HEAD, manifests, secrets, OSV, IaC), then optionally asks _your_ LLM — never a silent cloud fallback.

| It catches               | How                           |
| ------------------------ | ----------------------------- |
| Hallucinated imports     | npm / PyPI / crates.io HEAD   |
| Phantom deps             | Import vs manifest            |
| Secrets & vulns          | Pattern + OSV                 |
| IaC footguns             | Open CIDR, privileged pods    |
| Agent-shaped PRs         | Tier‑1 first, LLM nits capped |
| Blast radius / dep delta | Walkthrough + `@impact`       |

Fail-closed offline mode. Finding provenance. Learning from dismissals. Air-gap honest.

---

## Quick start

```bash
git clone https://github.com/lohitkolluri/codasaurus.git
cd codasaurus
docker compose up
```

Open the dashboard → finish the [onboarding wizard](docs/setup-onboarding.md) (~5 min) → install the GitHub App.

**Want $0 cloud hosting?** Follow [Run completely free](docs/run-for-free.md) (Render + Neon + optional free LLM).

```bash
# From source (Postgres must be reachable)
cargo build --release
export DATABASE_URL="postgres://codasaurus:codasaurus@127.0.0.1:5432/codasaurus"
./target/release/codasaurus serve --port 3000
```

```text
codasaurus serve | health | version
```

---

## Docs

| Guide                                                              | Topic                              |
| ------------------------------------------------------------------ | ---------------------------------- |
| [Setup — onboarding](docs/setup-onboarding.md)                     | First-run wizard                   |
| [Setup — GitHub App](docs/setup-github-app.md)                     | Permissions, manifest, manual keys |
| [Run completely free](docs/run-for-free.md)                        | Free host + Postgres + LLM stack   |
| [Database](docs/database.md)                                       | PostgreSQL, pool, schema           |
| [Configuration](docs/configuration.md)                             | Env vars, TOML, offline, OIDC      |
| [Commands](docs/commands.md)                                       | `@codasaurus` on PRs               |
| [Operations — backup & restore](docs/operations-backup-restore.md) | Postgres backup, HA, health        |

---

## On a PR

```text
@codasaurus review      @codasaurus describe     @codasaurus improve
@codasaurus security    @codasaurus impact       @codasaurus similar
@codasaurus ask …       @codasaurus fix          @codasaurus ignore <fp>
@codasaurus help
```

Also: `summarize` · `labels` · `changelog` · `add_docs` — full table in [docs/commands.md](docs/commands.md).

---

## vs the field

|                      | Seat SaaS   | PR-Agent         | **Codasaurus**                    |
| -------------------- | ----------- | ---------------- | --------------------------------- |
| Cost                 | Per seat    | OSS / commercial | **Self-host, free**               |
| Deploy               | Their cloud | Your infra       | **One Docker binary**             |
| Hallucinated imports | Rare        | Rare             | **Tier‑1**                        |
| LLM                  | Bundled     | BYOK / vendor    | **BYOK · fail-closed offline**    |
| Trust                | Opaque      | Varies           | **Provenance + dismiss learning** |

---

## LLM (optional)

```bash
export OPENROUTER_API_KEY="sk-or-..."
# or Ollama / any OpenAI-compatible endpoint
export CODASAURUS_BASE_URL="http://localhost:11434/v1"
export CODASAURUS_MODEL="qwen2.5-coder:7b"
```

Toggle per repo in the dashboard. `offline_mode` / `CODASAURUS_OFFLINE=1` never opens an LLM socket. Details: [configuration.md](docs/configuration.md).

---

## Config sketch

```toml
[checks]
hallucinated_imports = true
phantom_deps = true
vulnerabilities = true
secrets = true
iac = true
```

Repo overlays live in the dashboard (`config_json`). OIDC: `OIDC_ISSUER`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, `PUBLIC_URL`.

---

## Develop

```bash
cargo fmt --check
cargo clippy -- -D warnings
CODASAURUS_SKIP_FRONTEND_BUILD=1 cargo test
cargo build --release
```

---

MIT · [LICENSE](LICENSE) · [Lohit Kolluri](https://github.com/lohitkolluri)
