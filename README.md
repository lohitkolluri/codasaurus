<p align="center">
  <img src="assets/logo.svg" alt="Codasaurus" width="96">
</p>

<h1 align="center">Codasaurus</h1>

<p align="center">
  <b>Self-hosted GitHub App that reviews PRs like a senior who actually reads the diff.</b>
</p>

<p align="center">
  Tier-1 detectors first · optional BYOK LLM · zero seat tax · run for $0<br>
  <code>@codasaurus</code> on any PR · Svelte dashboard · PostgreSQL only
</p>

<p align="center">
  <a href="https://github.com/lohitkolluri/codasaurus/actions/workflows/ci.yml"><img src="https://github.com/lohitkolluri/codasaurus/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-1.88+-dea584?logo=rust&logoColor=white" alt="Rust"></a>
  <a href="docs/database.md"><img src="https://img.shields.io/badge/db-PostgreSQL-4169E1?logo=postgresql&logoColor=white" alt="PostgreSQL"></a>
  <a href="docs/run-for-free.md"><img src="https://img.shields.io/badge/host-run%20for%20%240-2ea44f" alt="Run for free"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/lohitkolluri/codasaurus" alt="License"></a>
</p>

<p align="center">
  <a href="docs/setup-onboarding.md"><img src="https://img.shields.io/badge/docs-onboarding-0ea5e9" alt="Onboarding"></a>
  <a href="docs/commands.md"><img src="https://img.shields.io/badge/docs-@codasaurus%20commands-8b5cf6" alt="Commands"></a>
  <a href="docs/configuration.md"><img src="https://img.shields.io/badge/docs-config-64748b" alt="Config"></a>
</p>

---

Agents ship volume. Models invent packages, paste last year's APIs, and leave secrets in the hunk. SaaS reviewers charge per seat and still miss the boring breaks.

**Codasaurus** is one Rust binary you run yourself. On every PR it proves what it can (registry HEAD, manifests, secrets, OSV, IaC), then optionally asks _your_ LLM. Never a silent cloud fallback.

| It catches               | How                           |
| ------------------------ | ----------------------------- |
| Hallucinated imports     | npm / PyPI / crates.io HEAD   |
| Phantom deps             | Import vs manifest            |
| Secrets and vulns        | Pattern + OSV                 |
| IaC footguns             | Open CIDR, privileged pods    |
| Agent-shaped PRs         | Tier-1 first, LLM nits capped |
| Blast radius / dep delta | Walkthrough + `@impact`       |

Fail-closed offline mode. Finding provenance. Learning from dismissals. Air-gap honest.

---

## Quick start

```bash
git clone https://github.com/lohitkolluri/codasaurus.git
cd codasaurus
docker compose up
```

Open the dashboard, finish the [onboarding wizard](docs/setup-onboarding.md) (~5 min), install the GitHub App.

Want **$0 forever** (no trials)? [Run on always-free infra](docs/run-for-free.md): Render free web + Aiven Free Postgres (or Neon). LLM optional / off.

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

| Guide                                                   | Topic                              |
| ------------------------------------------------------- | ---------------------------------- |
| [Onboarding](docs/setup-onboarding.md)                  | First-run wizard                   |
| [GitHub App](docs/setup-github-app.md)                  | Permissions, manifest, manual keys |
| [Run for free](docs/run-for-free.md)                    | Always-free host + Postgres        |
| [Database](docs/database.md)                            | PostgreSQL, pool, schema           |
| [Configuration](docs/configuration.md)                  | Env vars, TOML, offline, OIDC      |
| [`.codasaurus.toml` schema](docs/codasaurus-toml.md)    | In-repo config reference           |
| [Commands](docs/commands.md)                            | `@codasaurus` on PRs               |
| [Backup and restore](docs/operations-backup-restore.md) | Postgres backup, HA, health        |

Full index: [docs/README.md](docs/README.md).

---

## On a PR

```text
@codasaurus review      @codasaurus describe     @codasaurus improve
@codasaurus security    @codasaurus impact       @codasaurus similar
@codasaurus ask …       @codasaurus fix [fp]     @codasaurus ignore <fp>
@codasaurus help
```

Also: `summarize`, `labels`, `changelog`, `add_docs`. React 👎 on a finding comment to dismiss. Full table in [docs/commands.md](docs/commands.md).

---

## vs the field

|                      | Seat SaaS   | PR-Agent         | **Codasaurus**                    |
| -------------------- | ----------- | ---------------- | --------------------------------- |
| Cost                 | Per seat    | OSS / commercial | **Self-host, free**               |
| Deploy               | Their cloud | Your infra       | **One Docker binary**             |
| Hallucinated imports | Rare        | Rare             | **Tier-1**                        |
| LLM                  | Bundled     | BYOK / vendor    | **BYOK, fail-closed offline**     |
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
