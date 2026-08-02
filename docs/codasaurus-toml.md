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
lockfile_drift = true
license_drift = true
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

[quality_gate]
name = "codasaurus way"
block_on_fail = true

[[quality_gate.conditions]]
metric = "new_blocker_issues"
op = "gt"
threshold = 0.0

[[quality_gate.conditions]]
metric = "new_high_issues"
op = "gt"
threshold = 0.0

[[quality_gate.conditions]]
metric = "new_medium_issues"
op = "gt"
threshold = 5.0
```

## Sections

| Section        | Purpose                                                   |
| -------------- | --------------------------------------------------------- |
| `[checks]`     | Enable/disable Tier-1 detectors; `exclude_patterns` globs |
| `[behavior]`   | `strict`, `review_strictness`                             |
| `[registry]`   | Package registry / OSV cache TTL                          |
| `[guidelines]` | Contribution guideline path override                      |
| `[pre_merge]`  | Soft caps used as defaults before DB policy overlay       |
| `[quality_gate]` | Sonar-style gate on new findings; failed gate blocks the check run when `block_on_fail` |

## `review_strictness`

| Value      | Effect                                                         |
| ---------- | -------------------------------------------------------------- |
| `lenient`  | Raise floor toward warning; hide info; fewer warnings surfaced |
| `balanced` | Use `default_severity` + default signal budgets                |
| `strict`   | Surface more warnings; thorough LLM tone                       |
| `nitpick`  | Force info floor; wider info/warning budgets; nitpick LLM tone |

Dashboard **Settings → Review → Tone & thresholds** overrides TOML when set. Repo `config_json.policy.review_strictness` overrides both.

## `quality_gate`

Sonar-style gate evaluated against findings on new code lines. Any failed condition fails the gate; when `block_on_fail = true` the Codasaurus check run concludes `action_required`. Findings on pre-existing lines are baseline-suppressed (recorded once, then hidden) and do not count toward the gate.

| Metric                | Counts                |
| --------------------- | --------------------- |
| `new_issues`          | All new-code findings |
| `new_blocker_issues`  | Severity `blocking`   |
| `new_high_issues`     | Severity `blocking` (taxonomy has no `high`; maps to blocking) |
| `new_medium_issues`   | Severity `warning`    |
| `new_warning_issues`  | Severity `warning`    |
| `new_info_issues`     | Severity `info`       |

Operators: `gt`, `gte`, `lt`, `lte`, `eq`, `ne`.

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
    },
    "quality_gate": {
        "name": "release",
        "block_on_fail": true,
        "conditions": [
            { "metric": "new_blocker_issues", "op": "gt", "threshold": 0 },
            { "metric": "new_medium_issues", "op": "gt", "threshold": 2 }
        ]
    }
}
```

See also: [configuration.md](configuration.md), [commands.md](commands.md).
