# Commands

<p>
  <img src="https://img.shields.io/badge/@codasaurus-commands-8b5cf6" alt="Commands">
  <a href="README.md"><img src="https://img.shields.io/badge/docs-index-111827" alt="Docs index"></a>
</p>

Mention **`@codasaurus`** (or your App’s slug) on a pull request comment. Commands are case-insensitive; extra words after the verb become arguments for `ask` / `ignore` / `fix`.

## Core review

| Command     | What it does                                             |
| ----------- | -------------------------------------------------------- |
| `review`    | Full static (+ optional LLM) review                      |
| `describe`  | Walkthrough / PR summary                                 |
| `summarize` | Short executive summary                                  |
| `improve`   | Actionable improvement suggestions (LLM when configured) |
| `security`  | Secrets / vulnerability-focused scan                     |
| `help`      | Command reference                                        |

## Context & impact

| Command   | What it does                                               |
| --------- | ---------------------------------------------------------- |
| `impact`  | Blast-radius estimate for changed paths                    |
| `similar` | Related PRs by path history                                |
| `ask …`   | Answer a question about this PR                            |
| `digest`  | Weekly review rollup (same numbers as Dashboard analytics) |

## Repo hygiene

| Command                          | What it does                                                            |
| -------------------------------- | ----------------------------------------------------------------------- |
| `labels`                         | Suggest and apply PR labels                                             |
| `changelog` / `update_changelog` | Keep a Changelog–style draft                                            |
| `add_docs`                       | README / docs stubs (LLM)                                               |
| `fix` / `fix <fingerprint>`      | Apply available codemods (opt-in; needs Contents Write + settings flag) |
| `ignore <fingerprint>`           | Dismiss a finding by fingerprint (feeds learning)                       |

React **👎** or **confused** on a Codasaurus finding comment to dismiss that fingerprint (requires `reaction` webhook + Reactions: Read).

## Automatic behavior

On `opened` / `synchronize` (when enabled), Codasaurus runs describe+review without a mention: walkthrough, inline findings, optional Check Run, labels, and novelty sections (blast radius, dependency delta, agent badge, provenance).

## Examples

```text
@codasaurus review
@codasaurus describe
@codasaurus ask why is the cache keyed only on user id?
@codasaurus ignore a1b2c3d4
@codasaurus fix a1b2c3d4
@codasaurus digest
@codasaurus impact
```
