<p align="center">
  <img src="https://img.shields.io/badge/status-alpha-yellow" alt="Status">
  <img src="https://img.shields.io/github/license/lohitkolluri/codasaurus" alt="License">
  <img src="https://img.shields.io/badge/rust-1.97.1-blue" alt="Rust">
</p>

<h1 align="center">🦕 Codasaurus</h1>
<p align="center"><b>Munches on AI-generated bugs so you don't have to.</b></p>
<p align="center">
  Catches hallucinated imports, phantom deps, security holes, and over-engineered slop.<br>
  Works locally, in CI, and as a GitHub bot. Bring your own LLM key.
</p>

---

## The Problem

AI coding assistants write code fast. They also:

- **Hallucinate packages** — `import { magicSauce } from 'non-existent-package'`
- **Forget deps** — use lodash but never add it to `package.json`
- **Over-engineer** — factory pattern for 2 variants, 400-line functions
- **Leave TODOs** — `// TODO: implement proper error handling`
- **Leak secrets** — hardcoded API keys, tokens, connection strings
- **Use stale APIs** — trained on old docs, calls deprecated methods

Codasaurus catches all of this **before** it hits your repo.

## Two-Tier Architecture

```
                    ┌─────────────────────────────┐
                    │     codasaurus check         │
                    └──────────┬──────────────────┘
                               │
            ┌──────────────────┼──────────────────┐
            ▼                  ▼                   ▼
   ┌────────────────┐  ┌────────────────┐  ┌────────────────┐
   │  Tier 1: Static │  │  Tier 1: Static │  │  Tier 2: LLM   │
   │  Deterministic  │  │  Deterministic  │  │  (Optional)    │
   │                 │  │                 │  │                │
   │ • Hallucinated  │  │ • Secrets       │  │ • Logic bugs   │
   │   imports       │  │ • Credentials   │  │ • Security     │
   │ • Phantom deps  │  │ • OSV vulns     │  │ • API misuse   │
   │ • Over-engin.   │  │ • TODO leaks    │  │ • Edge cases   │
   │ • Boilerplate   │  │                 │  │ • Arch review  │
   └────────────────┘  └────────────────┘  └────────────────┘
            │                  │                    │
            └──────────────────┼────────────────────┘
                               ▼
                    ┌─────────────────────┐
                    │   Unified Report     │
                    │   (terminal / JSON)  │
                    └─────────────────────┘
```

### Tier 1: Static Detectors (Free, Deterministic)

| Detector | What it catches | Cost |
|----------|----------------|------|
| 🚫 **hallucinated-imports** | Non-existent npm/PyPI/crates.io packages | Free |
| 👻 **phantom-deps** | Packages used but not declared in deps files | Free |
| 🔑 **secrets** | API keys, tokens, passwords, JWTs, connection strings | Free |
| 📝 **todo-leaks** | `TODO`, `FIXME`, `XXX`, `HACK` placeholders left by AI | Free |
| 🏭 **over-engineering** | Factory patterns with 1-2 variants, unnecessary interfaces | Free |
| 📦 **boilerplate** | 200+ line functions, excessive getters/setters, repeated code | Free |
| 🛡️ **vulnerabilities** | OSV.dev database check for known package vulnerabilities | Free |

### Tier 2: LLM Review (Optional, BYOK)

Uses OpenRouter to send the diff to any LLM for deep semantic analysis. You bring your own API key — we don't charge per-seat.

| Feature | What it catches |
|---------|----------------|
| 🔒 Security | SQL injection, XSS, command injection, auth flaws |
| 🐛 Logic | Off-by-one, race conditions, missed edge cases, incorrect comparison |
| 🔧 API misuse | Wrong signatures, missing error handling, deprecated calls |
| 🎯 Maintainability | Dead code, needless complexity, architectural issues |
| ✅ Requirement validation | Verifies PR addresses linked issues (when context provided) |

**Supported models** (via OpenRouter's 400+ provider network):

```bash
export CODASAURUS_MODEL="anthropic/claude-sonnet-4.6"     # Default
export CODASAURUS_MODEL="anthropic/claude-opus-4.8"       # Best quality
export CODASAURUS_MODEL="google/gemini-3.1-flash-lite"     # Cheapest
export CODASAURUS_MODEL="openai/gpt-5.2"                  # OpenAI
export CODASAURUS_MODEL="deepseek/deepseek-v4"            # DeepSeek
export CODASAURUS_MODEL="openrouter/auto"                 # Auto-pick cheapest
```

## Quick Start

```bash
# Install
cargo install codasaurus

# Check staged changes (no setup, no config)
cd my-project
codasaurus check --staged

# Or run against a specific file
codasaurus check src/main.rs

# CI mode (JSON output, exits non-zero on issues)
codasaurus check --diff origin/main --ci

# With LLM review (requires API key)
export OPENROUTER_API_KEY="sk-or-..."
codasaurus check --staged --llm
```

### Usage

```bash
codasaurus [COMMAND]

Commands:
  check    Run verification on staged changes (default command)
  watch    Watch mode — live feedback as you code
  version  Print version information

Flags (for check):
  --staged          Check staged changes (default)
  --diff <REF>      Check diff against a git ref (e.g. --diff origin/main)
  --ci              CI mode — JSON output, strict exit codes
  --json            Output as JSON
  --llm             Enable LLM-powered deep review (requires API key)
  --config <PATH>   Path to .codasaurus.toml config file
  <PATH>            Check a specific file or directory
```

## Configuration

Create `.codasaurus.toml` in your project root:

```toml
[checks]
hallucinated_imports = true
phantom_deps = true
vulnerabilities = true
secrets = true
over_engineering = true
boilerplate = true
todo_leaks = true

[behavior]
default_severity = "warn"
strict = false

[registry]
timeout_ms = 5000
cache_ttl_secs = 3600
```

## GitHub Action

```yaml
# .github/workflows/codasaurus.yml
name: Codasaurus
on: [pull_request]
jobs:
  review:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Codasaurus Review
        uses: lohitkolluri/codasaurus@v1
        with:
          mode: ci
          diff: origin/main
```

## GitHub Bot (Coming Soon)

Codasaurus will also run as a GitHub App that:
- Posts inline review comments on PRs
- Reads linked issues to verify requirements
- Checks related PRs for context
- Supports BYOK for all LLM providers

## How It's Different

| | CodeRabbit | Greptile | Hawk / Duck | **Codasaurus** |
|---|---|---|---|---|
| **Price** | $24/seat/mo | $15+/seat/mo | Free/BYOK | **Free, open source** |
| **Local checks** | ❌ Cloud only | ❌ Cloud only | Partial | ✅ **Pre-commit + CLI** |
| **AI-specific detectors** | ❌ Generic review | ❌ Generic | Some | ✅ **Hallucinated imports, phantom deps** |
| **Multi-language** | JS/TS heavy | Limited | Varies | **10+ languages** via regex patterns |
| **Security scanning** | ✅ Paid tier | ✅ | ❌ | ✅ **OSV.dev + secrets (free)** |
| **LLM review** | ✅ Built-in | ✅ Built-in | ❌ | ✅ **BYOK via OpenRouter** |
| **Deterministic checks** | Some | ❌ | ❌ | ✅ **Zero false positives on Tier 1** |
| **PR context awareness** | ✅ Linked issues | ❌ | ❌ | ✅ **Linked issues + related PRs** |
| **Install time** | SaaS signup | SaaS signup | `npm install` | **`cargo install`** |

## Development

```bash
# Build
cargo build --release

# Run tests
cargo test

# Check staged changes in codasaurus itself
cargo run -- check --staged

# With LLM review
export OPENROUTER_API_KEY="sk-or-..."
cargo run -- check --staged --llm
```

## Tagline Generator

<details>
<summary>Possible taglines (pick your favorite)</summary>

- 🦕 *"Munches on AI-generated bugs so you don't have to."*
- 🦕 *"The code review dinosaur that eats your AI slop."*
- 🦕 *"Crunching AI bugs since 2026."*
- 🦕 *"Your AI wrote it. Codasaurus reviewed it. You shipped it."*
- 🦕 *"Hallucinated imports? Phantom deps? Let the dinosaur handle it."*

</details>

## License

MIT

---

<p align="center">
  <sub>Built with 🦕 by <a href="https://github.com/lohitkolluri">Lohit Kolluri</a></sub>
</p>
