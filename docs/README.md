# Docs

<p>
  <a href="../README.md"><img src="https://img.shields.io/badge/home-README-111827" alt="README"></a>
  <a href="https://github.com/lohitkolluri/codasaurus/actions/workflows/ci.yml"><img src="https://github.com/lohitkolluri/codasaurus/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="run-for-free.md"><img src="https://img.shields.io/badge/%240-always%20free-2ea44f" alt="Always free"></a>
  <a href="database.md"><img src="https://img.shields.io/badge/db-PostgreSQL-4169E1?logo=postgresql&logoColor=white" alt="PostgreSQL"></a>
</p>

Guides for running Codasaurus yourself. Pick a lane:

|                                                                    | Guide                                              | When                                    |
| :----------------------------------------------------------------: | -------------------------------------------------- | --------------------------------------- |
|  <img src="https://img.shields.io/badge/1-setup-0ea5e9" alt="1">   | [Onboarding wizard](setup-onboarding.md)           | Fresh install, first dashboard boot     |
|  <img src="https://img.shields.io/badge/2-setup-0ea5e9" alt="2">   | [GitHub App](setup-github-app.md)                  | Manifest flow, permissions, manual keys |
|   <img src="https://img.shields.io/badge/3-host-2ea44f" alt="3">   | [Run for free](run-for-free.md)                    | Aiven + Render (or Neon) at $0 forever  |
|   <img src="https://img.shields.io/badge/4-data-4169E1" alt="4">   | [Database](database.md)                            | Pool, schema, multi-replica             |
|   <img src="https://img.shields.io/badge/5-ops-64748b" alt="5">    | [Configuration](configuration.md)                  | Env, TOML, offline mode, OIDC           |
| <img src="https://img.shields.io/badge/5b-config-64748b" alt="5b"> | [`.codasaurus.toml` schema](codasaurus-toml.md)    | In-repo TOML reference                  |
|   <img src="https://img.shields.io/badge/6-prs-8b5cf6" alt="6">    | [Commands](commands.md)                            | `@codasaurus` on pull requests          |
|   <img src="https://img.shields.io/badge/7-ops-f59e0b" alt="7">    | [Backup and restore](operations-backup-restore.md) | `pg_dump`, HA, health                   |

### Fast paths

```text
compose up  →  onboarding  →  GitHub App  →  @codasaurus review
```

```text
Render free web  +  Aiven Free Postgres  →  run-for-free.md
```

Product overview stays in the root [README](../README.md).

### Project

| Doc | Topic |
| --- | --- |
| [CHANGELOG.md](../CHANGELOG.md) | Release notes |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | Dev setup and PR checks |
| [SECURITY.md](../SECURITY.md) | Vulnerability reporting |
| [.env.example](../.env.example) | Compose / local env knobs |
