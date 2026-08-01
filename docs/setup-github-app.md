# GitHub App setup

<p>
  <img src="https://img.shields.io/badge/setup-GitHub%20App-2088FF?logo=github&logoColor=white" alt="GitHub App">
  <a href="setup-onboarding.md"><img src="https://img.shields.io/badge/prefer-wizard-0ea5e9" alt="Wizard"></a>
  <a href="README.md"><img src="https://img.shields.io/badge/docs-index-111827" alt="Docs index"></a>
</p>

Codasaurus reviews PRs as a **GitHub App**. Prefer the dashboard [onboarding wizard](setup-onboarding.md) (manifest flow). Use this guide for manual registration or troubleshooting.

## Permissions

| Permission    | Access                                     | Why                                               |
| ------------- | ------------------------------------------ | ------------------------------------------------- |
| Pull requests | Read & Write                               | Reviews, comments, labels, requested reviewers    |
| Contents      | Read & Write (Write for `@codasaurus fix`) | Diffs, file fetch, optional branch writes         |
| Issues        | Read & Write                               | Linked-issue context, assessment                  |
| Checks        | Read & Write                               | Optional Check Runs                               |
| Reactions     | Read                                       | 👎 / confused on finding comments → learn dismiss |
| Metadata      | Read                                       | Required baseline                                 |

**Subscribe to events:** `Pull request` · `Issue comment` · `Reaction`

Webhook URL: `https://<your-host>/webhook` (trailing slash is also accepted).
Generate a secret: `openssl rand -hex 32`.

Logo (optional): upload [`assets/logo.png`](../assets/logo.png).

## Manifest flow (wizard)

1. In the wizard, click **Create GitHub App**.
2. GitHub opens a pre-filled form (`POST` manifest from `/api/setup/github/manifest-page`).
3. Confirm → GitHub redirects to `/api/setup/github/callback` with a one-time `code`.
4. Codasaurus exchanges the code, stores App ID / PEM / webhook secret / slug, and returns you to the wizard.

`PUBLIC_URL` (or the request Host) must match the URL GitHub can reach for webhooks and callbacks.

## Manual credentials

**Settings → Developer settings → GitHub Apps → New GitHub App**, then set:

| Field          | Value                                                             |
| -------------- | ----------------------------------------------------------------- |
| Name           | `codasaurus` (or yours)                                           |
| Homepage       | your deployment URL                                               |
| Webhook URL    | `https://<host>/webhook`                                          |
| Webhook secret | from `openssl rand -hex 32`                                       |
| Callback URL   | `https://<host>/api/auth/github/callback` (if using GitHub login) |

Generate a **private key** (`.pem`). Never commit it.

### Environment

```bash
export GITHUB_APP_ID="123456"
# PEM with literal newlines, or base64:
export GITHUB_APP_PRIVATE_KEY="$(cat codasaurus.pem)"
# export GITHUB_APP_PRIVATE_KEY_B64="$(base64 < codasaurus.pem)"
export GITHUB_WEBHOOK_SECRET="…"
```

The setup wizard and **Settings → GitHub** can store the same fields in the DB. Env vars win when both are set (see [configuration.md](configuration.md)).

## Install on repositories

**Install App** → choose account/org → select repositories.

Install URL (when slug is known):
`https://github.com/apps/<slug>/installations/new`

Also exposed as `github_install_url` on `GET /api/setup/status` and `GET /api/github/install-url` (authenticated).

## Verify

```bash
curl -s http://localhost:3000/health
```

Expect `"status":"ok"` and an `egress_profile`. Open a PR on an installed repo. Codasaurus should post a review within the webhook delivery window.

### Common failures

| Symptom                 | Check                                                        |
| ----------------------- | ------------------------------------------------------------ |
| 401 on webhook          | `GITHUB_WEBHOOK_SECRET` matches the App setting              |
| No reviews              | App installed on that repo; events include Pull request      |
| Manifest callback error | `PUBLIC_URL` / TLS / reverse-proxy Host headers              |
| `@codasaurus fix` fails | Contents **Write** permission + `allow_auto_fix` in settings |
