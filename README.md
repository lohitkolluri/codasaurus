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

```mermaid
flowchart TB
    subgraph CLI["codasaurus check --staged"]
        direction LR
        A[git diff<br/>--cached] --> B[file parser<br/>10+ languages]
    end

    subgraph Tier1["Tier 1 · Static Detectors"]
        direction TB
        HALLUC[🚫 hallucinated-imports] --> REGISTRY[npm / PyPI / crates.io<br/>registry lookup]
        PHANTOM[👻 phantom-deps] --> DEPS[dependency file<br/>cross-reference]
        SEC[🔑 secrets] --> REGEX[regex patterns]
        TODO[📝 todo-leaks] --> SCAN[line scan]
        OVER[🏭 over-engineering] --> AST[AST heuristics]
        VULN[🛡️ vulnerabilities] --> OSV[OSV.dev API]
    end

    subgraph Tier2["Tier 2 · LLM Review (optional — BYOK)"]
        direction TB
        DIFF[diff + context] --> OR[OpenRouter API<br/>400+ models]
        OR --> STRUCT[JSON Schema<br/>structured output]
        STRUCT --> VERDICT{verdict}
        VERDICT -->|ship| SHIP[✅ merge as-is]
        VERDICT -->|fix-before-ship| FIX[🔧 address issues]
        VERDICT -->|hold| HOLD[⛔ needs design<br/>discussion]
    end

    subgraph OUTPUT["Unified Report"]
        TB[terminal output<br/>colored, grouped by file]
        JSON_OUT[JSON output<br/>machine-readable]
    end

    B --> Tier1
    B -.->|--llm flag| Tier2
    Tier1 --> OUTPUT
    Tier2 --> OUTPUT
```

## What Codasaurus Catches

| Detector | What it catches | How | Cost | False Positives |
|----------|----------------|-----|------|-----------------|
| 🚫 **hallucinated-imports** | Imports that don't exist on npm/PyPI/crates.io | Lives HEAD request to registry API | Free | **Zero** — deterministic |
| 👻 **phantom-deps** | Packages used but not in `package.json`/`Cargo.toml` | Cross-refs imports vs dependency files | Free | **Zero** — deterministic |
| 🔑 **secrets** | API keys, tokens, passwords, JWTs, connection strings | Regex patterns for 15+ credential formats | Free | **~2%** — known patterns only |
| 📝 **todo-leaks** | `TODO`, `FIXME`, `XXX`, `HACK` left by AI | Line scan of staged changes | Free | **Zero** — exact match |
| 🏭 **over-engineering** | Factory pattern for 1-2 variants, unnecessary interfaces | AST heuristics | Free | **~5%** — heuristic-based |
| 📦 **boilerplate** | 200+ line functions, excessive getters, repeated blocks | Pattern matching | Free | **~5%** — heuristic-based |
| 🛡️ **vulnerabilities** | Known package vulnerabilities | OSV.dev API query | Free | **Zero** — database-backed |
| 🤖 **LLM review** | Security flaws, logic bugs, API misuse, architecture | OpenRouter → any LLM model | BYOK | **~5%** — model-dependent |

## Quick Start

```bash
# Install
cargo install codasaurus

# Check staged changes (no setup, no config)
cd my-project
codasaurus check --staged

# CI mode (JSON output, exits non-zero on issues)
codasaurus check --diff origin/main --ci

# With deep LLM review (bring your own key)
export OPENROUTER_API_KEY="sk-or-..."
codasaurus check --staged --llm

# Check a specific file or directory
codasaurus check src/main.rs
```

## Example Output

```diff
🦕 Codasaurus found 5 issue(s):
  3 blocking
  2 warnings

📁 src/app.js

  ✗ [hallucinated-imports]:1
    Package `non-existent-package` not found on npm.
    → This import will crash at runtime. AI coding assistants
      sometimes invent package names that don't exist.
    → Check the correct package name and install it.

  ✗ [secrets]:15
    Potential API Key detected: `sk-live-...abcd`
    → Hardcoded credentials in committed code.
    → Use environment variables and rotate this key.

  ✗ [phantom-deps]:22
    Package `lodash` is used but not declared in package.json.
    → AI added the import but forgot to add the dependency.
    → Run: npm install lodash

  ⚠ [todo-leaks]:8
    Leftover placeholder: "// TODO: implement error handling"

  ⚠ [over-engineering]:30
    Factory pattern detected with only 2 variants — unnecessary
    abstraction for this scope.
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
```

## LLM Review (Optional)

Bring your own API key — no per-seat fees. Codasaurus uses OpenRouter, so one key works with 400+ models.

```bash
# Set your API key
export OPENROUTER_API_KEY="sk-or-..."

# Pick a model (optional, defaults to claude-sonnet-4.6)
export CODASAURUS_MODEL="anthropic/claude-sonnet-4.6"
export CODASAURUS_MODEL="google/gemini-3.1-flash-lite"  # cheaper
export CODASAURUS_MODEL="anthropic/claude-opus-4.8"     # best quality

# Run with LLM review
codasaurus check --staged --llm
```

### What the LLM catches that static detectors miss

```mermaid
flowchart LR
    subgraph Static["Static Detectors"]
        A1[Known patterns]
        A2[Registry lookups]
        A3[Regex matches]
    end

    subgraph LLM["LLM Review"]
        B1[Logic bugs]
        B2[Security flaws]
        B3[API misuse]
        B4[Architecture issues]
        B5[Missing error handling]
        B6[Edge cases]
        B7[Mixed paradigms]
    end

    A1 ~~~ B1
    A2 ~~~ B2
    A3 ~~~ B3
```

The LLM provides context-aware analysis:
- *"The app will crash at startup with a module-not-found error"* instead of just *"Package not found"*
- Flags **mixed module systems** (ESM `import` + CJS `require`)
- Detects **missing error handling** around risky operations
- Catches **logical inconsistencies** across multiple changes

## How It's Different

| | CodeRabbit | Greptile | Hawk / Duck | **Codasaurus** |
|---|---|---|---|---|
| **Price** | $24/seat/mo | $15+/seat/mo | Free/BYOK | **Free, open source** |
| **Local checks** | ❌ Cloud only | ❌ Cloud only | Partial | ✅ **Pre-commit + CLI** |
| **AI-specific detectors** | ❌ Generic | ❌ Generic | Some | ✅ **Hallucinated imports, phantom deps** |
| **Multi-language** | JS/TS heavy | Limited | Varies | **10+ languages** |
| **Security (free)** | ✅ Paid tier | ✅ | ❌ | ✅ **OSV.dev + secrets** |
| **LLM review** | ✅ Built-in | ✅ Built-in | ❌ | ✅ **BYOK via OpenRouter** |
| **Deterministic** | Some | ❌ | ❌ | ✅ **Zero false positives on Tier 1** |
| **PR context** | ✅ Linked issues | ❌ | ❌ | ✅ **Linked issues + related PRs** |
| **Install** | SaaS signup | SaaS signup | `npm install` | **`cargo install`** |

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

## Development

```bash
# Build release binary
cargo build --release

# Run end-to-end test
mkdir -p /tmp/test && cd /tmp/test && git init
echo 'import { x } from "fake-pkg"' > test.js
codasaurus check --staged

# With LLM
export OPENROUTER_API_KEY="sk-or-..."
codasaurus check --staged --llm
```

## Architecture

```mermaid
graph TD
    subgraph CLI["codasaurus"]
        CLI_PARSE[clap argument parser]
        GIT_DIFF[git diff --cached]
        FILE_PARSER[file parser<br/>10+ languages]
        CONFIG[.codasaurus.toml]
    end

    subgraph DETECTORS["Detector Pipeline"]
        HALL[hallucinated-imports] --> CACHE[result cache]
        PHAN[phantom-deps] --> CACHE
        SEC_SEC[secrets] --> CACHE
        TODO_DET[todo-leaks] --> CACHE
        OVER_DET[over-engineering] --> CACHE
        BOIL[boilerplate] --> CACHE
    end

    subgraph LLM_PIPE["LLM Pipeline"]
        DIFF_TRUNC[diff truncation<br/>8K char limit]
        PROMPT[senior engineer<br/>system prompt]
        CONTEXT[ReviewContext<br/>repo / PR / issues]
        OR_API[OpenRouter API<br/>400+ models]
        JSON_SCHEMA[JSON Schema<br/>validation]
    end

    subgraph OUTPUT_RENDER["Output"]
        TERM[terminal renderer<br/>colored, grouped]
        JSON_OUT[JSON serializer]
        EXIT_CODES[exit code logic]
    end

    CLI_PARSE --> GIT_DIFF
    GIT_DIFF --> FILE_PARSER
    CONFIG --> DETECTORS
    FILE_PARSER --> DETECTORS
    FILE_PARSER -.-> LLM_PIPE
    DETECTORS --> CACHE
    DIFF_TRUNC --> PROMPT
    PROMPT --> CONTEXT
    CONTEXT --> OR_API
    OR_API --> JSON_SCHEMA
    CACHE --> OUTPUT_RENDER
    JSON_SCHEMA --> OUTPUT_RENDER
```

## License

MIT

---

<p align="center">
  <sub>Built with 🦕 by <a href="https://github.com/lohitkolluri">Lohit Kolluri</a></sub>
</p>
