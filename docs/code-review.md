# Code review practices (Codasaurus)

How Codasaurus reviews PRs — and how we keep this repo reviewable.

## Agent review standards

- **Signal over noise:** severity budgets, learning dismissals, and duplicate-SHA suppressions come first.
- **Actionable comments:** every blocking/warning finding should say _what_ and _how to fix_ (suggestion / codemod when possible).
- **Separate comment slots:** walkthrough, LLM summary, describe, and auto-improve never overwrite each other.
- **Tier-1 blockers:** secrets, vulns, and IaC privileged/open-CIDR stay high signal; OSV without version stays `info`.
- **Human override:** `@codasaurus ignore <fingerprint>` + dashboard dismiss feed learning rules.
- **Context before judgment:** linked GitHub issues, Jira/Linear keys, related PRs, and full-file fetch for critical paths.

## Slash command hygiene

| Command                  | Intent                             |
| ------------------------ | ---------------------------------- |
| `review`                 | Static (+ optional LLM) pass       |
| `describe` / `summarize` | Orientation, not nitpicks          |
| `improve`                | Concrete fix suggestions           |
| `ask …`                  | Answer a specific question         |
| `changelog` / `add_docs` | Release/docs drafts (comment only) |
| `security` / `labels`    | Focused scans / soft labels        |

## Repo maintainability (this codebase)

### Module map

| Area            | Path              | Responsibility                                        |
| --------------- | ----------------- | ----------------------------------------------------- |
| Webhook routing | `bot/mod.rs`      | Delivery dedup, event dispatch, PR locks              |
| Slash commands  | `bot/commands.rs` | Parse + handlers (`@codasaurus …`)                    |
| Queue worker    | `bot/worker.rs`   | Claim/lease, notify, inline fallback                  |
| Review pipeline | `bot/review/`     | Orchestration + GitHub/LLM/persist helpers            |
| Dual DB         | `db/`             | SQLite default; Postgres via `DATABASE_URL` + dialect |

### `bot/review/` layout

| File           | Role                                        |
| -------------- | ------------------------------------------- |
| `pipeline.rs`  | `review_pr` / `ReviewOptions` orchestration |
| `github.rs`    | Client, PR/files fetch, comment slots       |
| `findings.rs`  | Severity, merge, critical-path heuristics   |
| `llm.rs`       | Summary + auto-improve comments             |
| `persist.rs`   | Dashboard review/findings write path        |
| `reviewers.rs` | History-based reviewer suggestions          |

### Review checklist (PRs to this repo)

- [ ] Prefer small modules over 1k+ line files; keep orchestration thin.
- [ ] New detectors: unit tests + severity rationale; avoid info spam.
- [ ] DB SQL: use `db_*!` macros so SQLite and Postgres stay dual-compatible.
- [ ] Comment slots: never reuse `walkthrough` / `llm_summary` / `describe` / `auto_improve` for unrelated text.
- [ ] Secrets: redact before persist (`markdown::redact_secrets`); no tokens in logs.
- [ ] Tests: `CODASAURUS_SKIP_FRONTEND_BUILD=1 cargo test --lib`; optional `CODASAURUS_TEST_DATABASE_URL` for Postgres smoke.
- [ ] Clippy: `cargo clippy --lib -- -D warnings`.

### Ops

- `/metrics` for p95 latency, queue depth, FP proxy — see `docs/ops-backup-restore.md`.
- No Redis required for mid-scale queues (Postgres `SKIP LOCKED` / SQLite claim).
