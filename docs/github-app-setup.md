# GitHub App setup

## 1. Create the App

**Settings → Developer settings → GitHub Apps → New GitHub App**

| Field          | Value                       |
| -------------- | --------------------------- |
| Name           | `codasaurus` (or yours)     |
| Homepage       | your repo or deployment URL |
| Webhook URL    | `https://<host>/webhook`    |
| Webhook secret | `openssl rand -hex 32`      |
| Logo           | upload `assets/logo.png`    |

**Permissions**

- Pull requests — Read & Write
- Contents — Read (& Write if you use `@codasaurus fix`)
- Issues — Read & Write
- Checks — Read & Write
- Metadata — Read

**Events:** Pull request · Issue comment

Install on Any account (or Only this account).

## 2. Private key

Generate a private key under the App settings. Store the `.pem` securely — never commit it.

## 3. Install

**Install App** → pick org/user → select repos Codasaurus should review.

## 4. Run Codasaurus

```bash
docker compose up
# or
cargo build --release && ./target/release/codasaurus serve --port 3000
```

| Variable                                     | Required | Notes                                 |
| -------------------------------------------- | -------- | ------------------------------------- |
| `GITHUB_APP_ID`                              | yes      | Numeric App ID                        |
| `GITHUB_APP_PRIVATE_KEY`                     | yes      | Full PEM (literal newlines)           |
| `GITHUB_WEBHOOK_SECRET`                      | yes      | Same secret as registration           |
| `DATABASE_URL`                               | no       | SQLite default; or `postgres://…`     |
| `OPENROUTER_API_KEY` / `CODASAURUS_BASE_URL` | no       | BYOK LLM                              |
| `OIDC_*` / `PUBLIC_URL`                      | no       | SSO                                   |
| `CODASAURUS_OFFLINE`                         | no       | Fail-closed: no LLM / registry egress |

Dashboard setup wizard can also store App credentials.

## 5. Verify

```bash
curl -s http://localhost:3000/health
```

Expect JSON with `"status":"ok"` and an `egress_profile`. Open a PR on an installed repo — Codasaurus should post a review.
