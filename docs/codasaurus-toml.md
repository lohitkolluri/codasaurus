# `.codasaurus.toml` schema

<p>
  <img src="https://img.shields.io/badge/config-TOML-64748b" alt="TOML">
  <a href="README.md"><img src="https://img.shields.io/badge/docs-index-111827" alt="Docs index"></a>
</p>

Place `.codasaurus.toml` at the repo root (or set `CODASAURUS_CONFIG`). Dashboard DB settings overlay this file for detector toggles and policy. Per-repo `config_json` from the UI can further override.

## Full example

```toml
[checks]
hallucinated_imports = true
phantom_deps = true
vulnerabilities = true
secrets = true
over_engineering = true
boilerplate = true
stale_api = true
risky_patterns = true
todo_leaks = true
guidelines = true
graph = true
iac = true
exclude_patterns = ["vendor/", "dist/", "*.lock", "node_modules/"]

[behavior]
# CLI: non-zero exit on findings. Also maps to review_strictness=strict when unset.
strict = false
# Review personality: lenient | balanced | strict | nitpick
review_strictness = "balanced"

[registry]
cache_ttl_secs = 3600

[guidelines]
# Optional path relative to repo root (else auto-discovers CONTRIBUTING.md / AGENTS.md / …)
# contributing_guidelines = "docs/CONTRIBUTING.md"

[pre_merge]
require_description = false
require_title_convention = false
max_blocking = 0
max_warnings = 20
```

## Sections

| Section        | Purpose                                                   |
| -------------- | --------------------------------------------------------- |
| `[checks]`     | Enable/disable Tier-1 detectors; `exclude_patterns` globs |
| `[behavior]`   | `strict`, `review_strictness`                             |
| `[registry]`   | Package registry / OSV cache TTL                          |
| `[guidelines]` | Contribution guideline path override                      |
| `[pre_merge]`  | Soft caps used as defaults before DB policy overlay       |

## `review_strictness`

| Value      | Effect                                                         |
| ---------- | -------------------------------------------------------------- |
| `lenient`  | Raise floor toward warning; hide info; fewer warnings surfaced |
| `balanced` | Use `default_severity` + default signal budgets                |
| `strict`   | Surface more warnings; thorough LLM tone                       |
| `nitpick`  | Force info floor; wider info/warning budgets; nitpick LLM tone |

Dashboard **Settings → Review → Tone & thresholds** overrides TOML when set. Repo `config_json.policy.review_strictness` overrides both.

## Repo `config_json` (dashboard)

```json
{
    "detectors": { "secrets": true, "graph": false },
    "llm_enabled": true,
    "auto_describe": true,
    "auto_review_diff": false,
    "allow_auto_fix": false,
    "pr_title_fix": "off",
    "exclude_patterns": ["vendor/"],
    "policy": {
        "min_severity": "warning",
        "max_warnings": 20,
        "max_blocking": 0,
        "forbidden_paths": ["secrets/"],
        "request_reviewers": true,
        "create_check_run": true,
        "review_strictness": "strict"
    }
}
```

See also: [configuration.md](configuration.md), [commands.md](commands.md).
