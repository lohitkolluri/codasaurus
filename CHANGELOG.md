# Changelog

All notable changes to Codasaurus are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-02

First public release.

### Added

- Self-hosted GitHub App PR review agent (Rust + Axum + PostgreSQL)
- Tier-1 detectors: hallucinated imports, phantom deps, secrets, vulnerabilities (OSV), risky patterns, IaC, and more
- Optional BYOK LLM (OpenRouter / Ollama / custom OpenAI-compatible) with fail-closed offline mode
- `@codasaurus` slash commands on PRs (review, describe, improve, security, impact, ignore, fix, and more)
- Svelte dashboard: Reviews, Repos, Stats, Team, Settings, Audit log
- Settings side-nav with progressive disclosure (LLM, Review, Connections, System, Account, Learning)
- Review presets, strictness cards, and detector groups (Safety / Quality / Advanced)
- Durable review job queue (`FOR UPDATE SKIP LOCKED`)
- Learning from dismissed findings (ignore rules)
- Finding provenance and dismiss / FP proxy stats
- In-process `app_config` cache with TTL + write-through (`CODASAURUS_CONFIG_CACHE_TTL_SECS`)
- Docker Compose and Render free-tier deploy path
- Linux `x86_64` release tarball via GitHub Actions on `v*` tags

### Security

- AGPL-3.0-or-later with attribution notice
- Non-root container image, hardened Compose defaults
- Session auth, RBAC (owner / maintainer / viewer), optional OIDC

[0.1.0]: https://github.com/lohitkolluri/codasaurus/releases/tag/v0.1.0
