# Security Policy

The Codasaurus maintainers take security seriously. This document describes which versions we support, how to report vulnerabilities, what is in scope, and what you can expect after reporting.

## Supported versions

Security fixes are applied to the latest release line. Older tags are not generally backported unless a fix is trivial and widely deployed.

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1   | :x:                |

Always prefer the newest [GitHub Release](https://github.com/lohitkolluri/codasaurus/releases).

## Reporting a vulnerability

**Do not** report security issues through public GitHub Issues, Discussions, or pull requests.

### Preferred: GitHub Private Vulnerability Reporting

If enabled for this repository, use GitHub’s private reporting flow:

**[Report a vulnerability](https://github.com/lohitkolluri/codasaurus/security/advisories/new)**

This keeps the report confidential, attaches it to a draft Security Advisory, and lets us coordinate disclosure without exposing details publicly.

> Maintainers: enable **Private vulnerability reporting** under  
> Repository → Settings → Code security → Private vulnerability reporting  
> so the link above works for researchers.

### Fallback: email

If private reporting is unavailable or unsuitable for your case, email:

**lohitkolluri@gmail.com**

Subject prefix (recommended): `[SECURITY] Codasaurus`

Encrypt sensitive attachments if you can; say so in the email if you need a PGP key or alternate channel.

### What to include

Reports are much more useful when they include:

1. **Affected version** — tag, commit SHA, or `codasaurus version` output  
2. **Description** — what the issue is and why it is security-relevant  
3. **Impact** — confidentiality, integrity, availability; who can exploit it (unauthenticated, logged-in viewer, maintainer, etc.)  
4. **Reproduction** — clear steps, minimal PoC, or a private test repo  
5. **Environment** — self-hosted Compose, Render, binary path, Postgres, reverse proxy  
6. **Mitigations** — anything you already tried or recommend  
7. **Credit** — how you want to be credited (name, handle, or anonymous)

Incomplete reports are still welcome; we may ask follow-up questions.

## Scope

### In scope

- The Codasaurus binary, HTTP API, and Svelte dashboard as shipped in this repository  
- Authentication / session handling, RBAC, and OIDC integration bugs that escalate privilege or bypass auth  
- Injection, SSRF, path traversal, or unsafe deserialization in request or webhook handling  
- Secrets exposure through logs, API responses, or default Compose configuration that is unsafe for production without operator change  
- Supply-chain issues in our release artifacts (tampered tarball, CI compromise indicators)

### Out of scope (usually)

- Denial of service that requires unrealistic resources or is inherent to self-hosted capacity limits  
- Issues that only affect misconfigured deployments (e.g. publicly exposing Postgres, sharing `GITHUB_APP_PRIVATE_KEY`, disabling auth)  
- Vulnerabilities in third-party services we integrate with (GitHub, OpenRouter, Aiven, Neon, Render, IdPs)—report those to the vendor  
- Findings that require physical access, or already-compromised host / admin credentials  
- Social engineering of maintainers or users  
- Theoretical issues without a plausible exploit path  

If you are unsure, report it privately anyway.

## Our commitments

| Expectation | Target |
| ----------- | ------ |
| Acknowledgement | Within **3 business days** |
| Triage / initial assessment | Within **7 business days** when possible |
| Status updates | Reasonable progress notes until fix or decline |
| Coordinated disclosure | We prefer fixing before public disclosure; typical window **90 days** from acknowledgement, adjustable by severity and complexity |

We may:

- Ask for a clearer reproducer or a CVE-style write-up  
- Invite you as a temporary collaborator on a private advisory  
- Credit you in the advisory / [CHANGELOG](CHANGELOG.md) `Security` section (unless you opt out)  
- Decline reports that are out of scope, duplicates, or not security issues—we will say why  

We will **not** pursue legal action against researchers who:

- Act in good faith  
- Avoid privacy violations, data destruction, and service disruption beyond what is needed to demonstrate the issue  
- Keep details private until we agree on disclosure (or the coordination window expires)

## Disclosure process

1. Report received (PVR or email)  
2. Maintainer acknowledges and triages  
3. Fix developed on a private branch / advisory when needed  
4. Patch released (patch version under SemVer when appropriate)  
5. Advisory / CHANGELOG `Security` notes published  
6. Public discussion welcome **after** the fix is available  

Critical issues may get an earlier public notice if operators need to take immediate action (rotate secrets, restrict network exposure, etc.).

## Operator hardening (baseline)

Self-hosting means you own the threat model. At minimum:

- Keep `DATABASE_URL`, GitHub App private keys, webhook secrets, and LLM API keys out of public repos and client-side code  
- Set a real `PUBLIC_URL` (HTTPS) in production; enable HSTS when terminating TLS  
- Restrict dashboard exposure (VPN, SSO, IP allowlists) for internet-facing instances  
- Prefer OIDC over shared passwords when available  
- Run the published non-root container / Compose defaults; do not run the binary as root  
- Review [docs/configuration.md](docs/configuration.md) and [docs/operations-backup-restore.md](docs/operations-backup-restore.md)

## Preference for safe reporting

We welcome responsible disclosure. Thank you for helping keep Codasaurus and its operators safe.
