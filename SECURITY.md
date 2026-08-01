# Security Policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security problems.

Email **lohitkolluri@gmail.com** with:

- Description of the issue
- Steps to reproduce (or PoC)
- Impact assessment if known
- Whether you want credit in a fix release

You should get an acknowledgement within a few days. We will coordinate a fix and disclosure timeline.

## Scope notes

- Self-hosted deployments: keep `DATABASE_URL`, GitHub App private keys, and LLM API keys out of public repos
- Report issues in Codasaurus itself (binary, dashboard, default Compose hardening)
- Third-party services (GitHub, Aiven, Neon, Render, OpenRouter) have their own reporting channels
