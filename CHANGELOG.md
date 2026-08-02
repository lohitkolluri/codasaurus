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
- PR reviews post three lean issue-comment slots: overview (prose + Changes table), optional context (gated Mermaid sequence diagram, blast, related PRs, deps), and pre-merge checks / effort / reviewers.
- `GET /branding/logo.png` serves the bundled App mark for post-wizard badge upload.
- `GET /branding/review-banner.svg` for dashboard branding when `PUBLIC_URL` is set.
- Review detail page: KPI strip, detector chips, severity filters, full file paths, suggestions, and richer metadata.

### Changed

- LLM `review_diff` prompts use a scoped 5-part framework (role, priority scope, do-not-report noise list, confidence/severity rules, structured output) to cut style false positives.
- Review JSON schema constrains `verdict` enum, requires `rationale` + `suggestion`, and caps issues at 8.
- PR title/description/issues in LLM context are wrapped in `<<<UNTRUSTED_*>>>` markers; default LLM diff budget raised to 24k chars (aligned with auto-improve).
- Finding filter order: slop + forbidden paths → severity floor → count caps → signal budget (policy no longer bypasses floors/budgets incorrectly).
- Forbidden path matching is exact/prefix/basename only (no substring false positives).
- Overview opens with shields status strip (`blocking|N` `warning|N` `info|N` `ready to merge|yes/no`), then short prose + **What to do next** checklist + Changes table.
- Re-reviews show **Since last review** (resolved / still open / new) by diffing against the previous completed PR review in the DB.
- Checks stay quiet when green (“All clear”); when not, unchecked items include how to pass.
- Inline findings lead with **Do this**; severity shields removed; fingerprints / ignore / fix tucked under details.
- Context sequence diagrams are LLM-gated (cheap model), sanitized, and abstain when useless — no folder flowchart fallback.
- Inline findings use Impact / Action / Evidence (collapsed) instead of long free-form dumps.
- Advisory bots no longer auto-`APPROVE` clean PRs — walkthrough + COMMENT/REQUEST_CHANGES reviews only.
- Signal budget drops advisory (`info`) inlines by default (high-signal blocking/warning only).
- Related / similar PRs: sample high-signal source paths (skip changelog/docs/lockfile noise), rank by overlap strength instead of GitHub file-list order.
- Durable `@codasaurus` command replies (describe, summarize, improve, ask, …) update their issue-comment slots in place instead of stacking new posts.
- Check Runs update in place on the same SHA.
- `@codasaurus review` force-reclaims the head SHA so same-commit re-runs are not silently skipped.
- Repo settings: removed no-op “Auto-describe on open” (walkthrough already covers open/push).
- Settings → System clarifies that Offline mode is a kill-switch (not tied to LLM keys) and shows when `CODASAURUS_OFFLINE` env forces it.
- Settings tabs sync to the URL (`/app/settings/llm|review|…`) so deep links stay accurate.
- Softened third-party branding in comments and operator copy: competitor review-bot names removed; host/LLM references use generic terms where a specific vendor is not required for configuration.
- Ignore `issue_comment` events from GitHub Apps/bots so review footers that mention `@codasaurus …` no longer trigger a self ACL-denial notice.
- Dashboard UI: shared thin scrollbar on `page-panel-scroll`, scrollable recent activity + review findings, GitHub line links for dismiss decisions, colored detector bars with padding, and quieter hover (no black-on-black).
- Overview comments include a collapsed copy-paste prompt for an AI coding agent when there are findings.
- Context blast radius uses shields badges (`BLAST RADIUS` / `SCORE`); low-noise blasts stay hidden.
- LLM PR summary prompt tightened and hard-capped (~600 chars) so comments stay scannable.
- Review maturity: advisory draft for soft findings, opt-in `auto_approve` on clean PRs (merge still needs a maintainer), concern labels (`security|quality|tests|docs`), LLM path/symbol grounding, learning promotion requires distinct PRs or a maintainer dismiss, and golden detector fixtures in CI.
- `.env.example` documents the full dashboard ↔ env mirror set (timeouts, cookies, model cheap, etc.).
- Jira / Linear ticket context is taken from the PR **title and body**; Linear issues resolve via `issue(id:)` (supports bare `ENG-123` when Linear is configured).
- Jira Cloud ADF descriptions are flattened into text for review context.
- Automatic reviews no longer post a duplicate “describe” comment (walkthrough stays on the review).
- Reviews list layout: title + status on one row, cleaner filters alignment.
- Bot commands accept plain `codasaurus …` / `/codasaurus …` (GitHub Apps are not @-mentionable).
- GitHub App manifest includes a description; wizard documents manual icon upload (manifest cannot set logos).

### Removed

- Dead `markdown::walkthrough_body` / `clean_approve_body_ext` wrappers, folder Mermaid change-map, retired `guidelines::detect` stub, and unused `Config::bot_policy` helper.
- Placeholder “configure JIRA\_\*” linked-issue stubs from walkthroughs (only real tickets when integrations are configured).

### Fixed

- Empty or fully-excluded PRs complete the SHA claim instead of leaving `in_progress` leases that skip later reviews.
- Hard 8k second truncate in `build_review_prompt` no longer ignored larger `CODASAURUS_MAX_LLM_DIFF_CHARS` / auto-improve budgets.
- Learning dismissals: short fingerprint prefixes require ≥12 chars and only match as a prefix of the finding fingerprint (no bidirectional collisions).
- Walkthrough summary is always updated in place on new commits (issue-comment slot); PR Reviews only carry short bodies + inlines — no longer a full duplicate walkthrough per push.
- Queue / SHA stale reclaim window tracks review timeout (+120s) so a second worker cannot steal a still-running job.
- Same-SHA skip now posts a short status comment explaining how to force a re-run.
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
