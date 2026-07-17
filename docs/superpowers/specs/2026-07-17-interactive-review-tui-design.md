# Interactive Review TUI — Design Spec

**Date:** 2026-07-17
**Status:** Draft
**Author:** Lohit Kolluri

## Overview

Add an interactive terminal UI to `codasaurus check` that lets users navigate findings, dismiss (ignore) them, open the file in an editor, and prompt AI with context — all without leaving the terminal.

Trigger: automatic in TTY, bypassed with `--json`, `--ci`, or pipe/redirect.

## Architecture

### New file
- `src/interactive.rs` — all TUI code. Uses `ratatui` + `crossterm`.

### Modified files
- `src/main.rs` — after `run_check`, if in TTY and not `--json`/`--ci`, branch to `interactive::run(findings, &config)` instead of `output::render()`
- `src/lib.rs` — add `pub mod interactive;`
- `Cargo.toml` — add `ratatui`, `crossterm`
- `src/config.rs` — add optional `[tui]` section with `editor` override field

### Untouched
- `output.rs` — still used for `--json`/`--ci`/piped
- `cli.rs` — `run_check` already returns `Findings`
- `detectors/` — TUI only displays, doesn't detect
- `learning/store.rs` — `dismiss()` API already exists, used as-is

## Screen Layout

```
┌─ Codasaurus Review ── 8 findings (5 blocking) ──┐
├────────────────┬────────────────────────────────┤
│ src/           │ ✗ hallucinated-imports [:1]   │
│   app.js  (4)  │   Package `fake-pkg` not...    │
│   utils.ts (2) │                                │
│                │ ✗ secrets [:4]                 │
│                │   Potential API Key: `sk...`   │
│                │                                │
│                │ ⚠ todo-leaks [:8]              │
│                │   Leftover: "// TODO: ..."     │
├────────────────┴────────────────────────────────┤
│ ↑↓ nav  | o open  | d dismiss  | p AI  | q    │
└─────────────────────────────────────────────────┘
```

- **Left pane:** file tree with finding counts per file, grouped by directory
- **Right pane:** findings for the selected file, one per row
- **Bottom bar:** always-visible keybinding hints
- **Colors:** ✗ red (blocking), ⚠ yellow (warning), ℹ cyan (info), dimmed grey (dismissed)
- **Resize:** ratatui handles terminal resize gracefully

## Keybindings

| Key | Action |
|-----|--------|
| `↑`/`↓`, `j`/`k` | Navigate findings |
| `Enter` | Expand/collapse detail (evidence, codemod) |
| `o` | Open `$EDITOR <file>:<line>` |
| `d` | Dismiss (ignore) — persists to `LearningStore`, dims in list |
| `p` | Open inline prompt bar → type question → copy to clipboard |
| `q` | Quit |
| `?` | Toggle help overlay |

## Actions Detail

### Open in editor (`o`)
- Uses `$EDITOR` env var, then `$VISUAL`, then configured `[tui].editor` in `.codasaurus.toml`
- Opens file at the finding's line number
- Falls back to `$EDITOR` with file:line notation supported by Vim, VS Code, Emacs, Nano

### Dismiss (`d`)
- Calls `LearningStore::dismiss()` — persists fingerprint to SQLite
- Finding visually dims immediately in the list
- Persisted across runs: `LearningStore::filter_findings()` already excludes dismissed items

### Prompt AI (`p`)
- Opens an inline text input bar at the bottom
- User types a question about the finding
- On submit, copies to clipboard: the finding's file + line + message + user's question
- Portable — works with any AI tool the user pastes into

### Quit (`q`)
- Exits TUI
- If dismissals were made, prints: `"N findings dismissed, will be hidden on future runs."`

## Edge Cases

| Case | Behavior |
|------|----------|
| No findings | Clean screen: `✓ No issues found. Press q to exit.` |
| Terminal < 80x24 | Warning + fallback to `output::render()` |
| Pipe/redirect (`|`, `>`)| Automatically bypasses TUI (uses regular output) |
| Editor not found | Warning printed, stays in TUI, no crash |
| Dismiss persistence fails | Warning printed, finding stays visible |
| Config has no `[tui]` section | Uses `$EDITOR` only — no config needed |

## Dependencies

- `ratatui` — terminal UI framework (buffer, widgets, rendering)
- `crossterm` — terminal backend (raw mode, events, resize)

## Files Changed

```
M  Cargo.toml          # +2 deps
M  src/lib.rs           # +1 line (pub mod interactive)
M  src/main.rs          # ~15 lines (TTY branch + interactive call)
A  src/interactive.rs   # ~500 lines (all TUI code)
A  docs/superpowers/specs/2025-07-17-interactive-review-tui-design.md
```
