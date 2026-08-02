# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## How to read this file

- **Added** for new features
- **Changed** for changes in existing functionality
- **Deprecated** for soon-to-be removed features
- **Removed** for now removed features
- **Fixed** for any bug fixes
- **Security** for vulnerability fixes and hardening that affect operators

Dates are UTC calendar days. Links at the bottom compare tags on GitHub.

---

## [Unreleased]

### Added

- CLI `codasaurus reset-password --email … --password …` for emergency local dashboard recovery (no email flow).
- Repository detail: **Remove from Codasaurus** (cascades local review history; does not uninstall the GitHub App).
- Settings → Connections: **Test connection** for GitHub App, OIDC discovery, Jira, and Linear; **Clear** for SSO and ticket credentials.
- GitHub App manifest now requests `installation` and `installation_repositories` events so installs sync without a manual Sync.
- PR review comments use shields.io verdict / severity badges and a structured walkthrough.
- Walkthrough sections: estimated effort, Mermaid change map, grouped file summary, linked-issue assessment, pre-merge checks.
- `GET /branding/logo.png` serves the bundled App mark for post-wizard badge upload.
- Review detail page: KPI strip, detector chips, severity filters, full file paths, suggestions, and richer metadata.

### Changed

- Related / similar PRs: sample high-signal source paths (skip changelog/docs/lockfile noise), rank by overlap strength instead of GitHub file-list order.
- Settings → System clarifies that Offline mode is a kill-switch (not tied to LLM keys) and shows when `CODASAURUS_OFFLINE` env forces it.
- Settings tabs sync to the URL (`/app/settings/llm|review|…`) so deep links stay accurate.
- Agent-authored PR badge reasons are generic (no vendor product names in walkthrough copy).
- `.env.example` documents the full dashboard ↔ env mirror set (timeouts, cookies, model cheap, etc.).
- Jira / Linear ticket context is taken from the PR **title and body**; Linear issues resolve via `issue(id:)` (supports bare `ENG-123` when Linear is configured).
- Jira Cloud ADF descriptions are flattened into text for review context.
- Automatic reviews no longer post a duplicate “describe” comment (walkthrough stays on the review).
- Reviews list layout: title + status on one row, cleaner filters alignment.
- Bot commands accept plain `codasaurus …` / `/codasaurus …` (GitHub Apps are not @-mentionable).
- Mermaid change map always included in review walkthroughs (not gated on LLM).
- GitHub App manifest includes a description; wizard documents manual icon upload (manifest cannot set logos).

### Deprecated

- Nothing yet.

### Removed

- Placeholder “configure JIRA_*” linked-issue stubs from walkthroughs (only real tickets when integrations are configured).

### Fixed

- Docker image build failed: `.dockerignore` excluded `assets/logo.png` required by `include_bytes!` for `/branding/logo.png`.
- Runtime GitHub App auth now accepts raw `GITHUB_APP_PRIVATE_KEY` everywhere (not only `*_B64`).
- Hallucinated-import suggestions link to real registry URLs (npmjs / PyPI / crates.io).
- Blank metrics token on save clears `/metrics` auth; insecure/secure cookie flags stay consistent.
- OIDC allow-flags from the dashboard (`true`/`false`) were ignored at runtime (only `1` was accepted).
- GitHub App manifest `callback_urls` pointed at a non-existent GitHub user-OAuth callback; they now match the setup callback.
- Dashboard / Stats pass-rate queries failed on Postgres (`NUMERIC` vs `FLOAT8`) — cast `AVG` to `float8`.
- Dashboard LLM keys now mirror into process env so `@codasaurus ask` sees Settings → LLM without a restart.
- Same-SHA review jobs no longer re-queue after success (duplicate webhook deliveries).
- Review detail API crashed decoding `findings.line_start` (`INT4` vs `INT8`) — Rust types and UNNEST casts aligned to `INTEGER`.
- `@codasaurus` commands also allow prior **contributors**, the **PR author**, and the **repo owner** by login; denied commands get an explanatory PR reply.

### Security

- Jira base URL and OIDC issuer are validated with the existing SSRF guards (including DNS checks) before save or outbound fetch.

---

## [0.1.0] - 2026-08-02

First public release of Codasaurus: a self-hosted GitHub App that reviews pull requests with Tier-1 detectors first, optional BYOK LLM, and a Svelte operations dashboard.

### Added

#### Core review pipeline

- Durable PR review job queue on PostgreSQL (`FOR UPDATE SKIP LOCKED`) with worker concurrency controls
- Tier-1 detectors that run without an LLM: hallucinated imports, phantom dependencies, secrets, OSV vulnerabilities, risky patterns, IaC heuristics, and related checks
- Optional BYOK LLM path (OpenRouter, Ollama, or any OpenAI-compatible base URL) with hard fail-closed offline mode (`CODASAURUS_OFFLINE` / dashboard toggle)
- Finding provenance metadata and dismiss → learn ignore rules (`Learning` settings)
- `@codasaurus` slash commands on PRs, including `review`, `describe`, `improve`, `security`, `impact`, `similar`, `ask`, `fix`, `ignore`, `digest`, `help`, and related helpers

#### Dashboard

- Svelte SPA: Dashboard, Stats, Repositories, Reviews, Team, Settings, Audit log
- Settings categories with progressive disclosure: LLM, Review, Connections, System, Account, Learning
- Review UX: detector groups (Safety / Quality / Advanced), presets, strictness choice cards, PR action toggles
- Stats: week-over-week KPIs, Chart.js trends, outcomes, detector mix with pagination
- Team invites and RBAC roles: owner, maintainer, viewer (bootstrap superuser)

#### Operations & packaging

- PostgreSQL-only persistence with automatic migrations (`schema_version` through v15)
- Docker multi-stage image, Compose stack (hardened defaults), Render free-tier blueprint
- In-process `app_config` cache with TTL and write-through (`CODASAURUS_CONFIG_CACHE_TTL_SECS`)
- Env ↔ dashboard mirrors for common tunables (see [docs/configuration.md](docs/configuration.md))
- Linux `x86_64` release tarball + SHA-256 via GitHub Actions on `v*` tags
- `.env.example`, onboarding and ops docs under [`docs/`](docs/README.md)

### Security

- Session authentication with role-based access control
- Optional OIDC SSO for dashboard login
- Non-root container user, read-only root filesystem options in Compose
- Licensed under [AGPL-3.0-or-later](LICENSE) with required attribution (AGPL §7)

### Known limitations

- Release binary targets `x86_64-unknown-linux-gnu` only (macOS / Windows / GHCR image deferred)
- No crates.io publish in the release workflow yet
- Email password reset is not implemented; use `codasaurus reset-password` or store the bootstrap owner password safely

---

[Unreleased]: https://github.com/lohitkolluri/codasaurus/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/lohitkolluri/codasaurus/releases/tag/v0.1.0
