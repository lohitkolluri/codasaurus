# Codasaurus — LLM Review Engine + Polish

**Status**: Plan  
**Author**: Prometheus  
**Date**: 2026-07-17  

---

## Objective

Implement the OpenRouter-based LLM review engine (Tier 2) with structured JSON output, update branding, and polish the codebase for first public release.

---

## Files to Create

### 1. `src/llm/mod.rs` — LLM Review Engine

**Module structure:**
- `LlmConfig` struct — `api_key`, `model`, `max_tokens`, `temperature`, `base_url`
- `LlmReviewOutput` struct — `verdict`, `issues: Vec<LlmIssue>`, `summary`
- `LlmIssue` struct — `severity`, `category`, `file`, `line`, `description`, `suggestion`, `confidence`
- `review_schema()` — returns JSON Schema for structured output
- `review_diff(diff, config)` — sends diff to OpenRouter, parses structured response
- `build_review_prompt(diff)` — constructs concise review prompt, truncates large diffs at 8000 chars
- `LlmConfig::from_env()` — reads `CODASAURUS_API_KEY` or `OPENROUTER_API_KEY` from env

**API**: OpenRouter `/v1/chat/completions` with `response_format: json_schema`

### 2. `src/detectors/llm.rs` — LLM Detector Wrapper

- `detect(parsed_files, config)` — collects diffs, calls `llm::review_diff()`, converts `LlmIssue` → `Finding`
- Integrates with existing `detectors::run_all()` via `config.checks.llm_review` flag

### 3. `src/config.rs` — Add LLM Config

- Add `llm: LlmConfig` to Config struct
- Add `llm_review: bool` to CheckConfig (default: false — opt-in)
- Read `CODASAURUS_API_KEY` env var

### 4. Update `src/detectors/mod.rs`

- Add `pub mod llm;`
- Add `llm_review` check in `run_all()` when config is provided

### 5. Update `Cargo.toml`

- Description: `🦕 Codasaurus — munches on AI-generated bugs so you don't have to. Catches hallucinated imports, phantom deps, security holes, and over-engineered slop. Works locally, in CI, and as a GitHub bot.`

### 6. `README.md` — Full README

- Catchy headline with dinosaur theme
- Screenshot/demo of CLI output
- Quick start: `cargo install codasaurus && codasaurus check --staged`
- Features list: Tier 1 (static) + Tier 2 (LLM) architecture
- Configuration guide
- GitHub Action badge

### 7. `src/main.rs` — Add `CODASAURUS_API_KEY` to version info

---

## Implementation Order

1. `src/llm/mod.rs` — core LLM engine (most critical, most complex)
2. `src/detectors/llm.rs` — LLM detector integration
3. Update `src/config.rs` — LLM config fields
4. Update `src/detectors/mod.rs` — wire LLM into pipeline
5. Update `Cargo.toml` — new description
6. `README.md` — full docs
7. Test compilation + fix errors
8. Commit + push

---

## Dependencies (no new crates needed)

- Uses `reqwest` + `serde_json` already in Cargo.toml — no additional dependencies
- Uses JSON Schema structured output via OpenRouter API

---

## Token Optimization Strategy

| Optimization | Method |
|---|---|
| Diff truncation | Cap at 8000 chars (handles 95%+ of PRs) |
| Only send changed lines | Not full files |
| Structured output | JSON Schema ensures deterministic parsing |
| No conversation history | Each review is a single stateless API call |
| Temperature 0.1 | Near-deterministic output |

---

## Execution

Worker should:
1. Create `src/llm/mod.rs`
2. Create `src/detectors/llm.rs`
3. Edit `src/config.rs` to add LLM fields
4. Edit `src/detectors/mod.rs` to wire LLM
5. Edit `Cargo.toml` description
6. Write `README.md`
7. `cargo check` + fix
8. `git add -A && git commit -m "feat: llm review engine via OpenRouter" && git push`
