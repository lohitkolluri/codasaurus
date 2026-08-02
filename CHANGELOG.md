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

- Settings → Connections: **Test connection** for GitHub App, OIDC discovery, Jira, and Linear; **Clear** for SSO and ticket credentials.
- GitHub App manifest now requests `installation` and `installation_repositories` events so installs sync without a manual Sync.

### Changed

- Jira / Linear ticket context is taken from the PR **title and body**; Linear issues resolve via `issue(id:)` (supports bare `ENG-123` when Linear is configured).
- Jira Cloud ADF descriptions are flattened into text for review context.

### Deprecated

- Nothing yet.

### Removed

- Nothing yet.

### Fixed

- OIDC allow-flags from the dashboard (`true`/`false`) were ignored at runtime (only `1` was accepted).
- GitHub App manifest `callback_urls` pointed at a non-existent GitHub user-OAuth callback; they now match the setup callback.

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
- Email password reset is not implemented; store the bootstrap owner password safely

---

[Unreleased]: https://github.com/lohitkolluri/codasaurus/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/lohitkolluri/codasaurus/releases/tag/v0.1.0
