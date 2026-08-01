# Setup — onboarding wizard

Codasaurus serves a Svelte SPA on the same port as the API. On a fresh install, open the dashboard and finish the wizard (~5 minutes). You can leave and resume; progress is derived from stored config, not a sticky “wizard state” cookie.

## Steps

| #   | Step                | Required          | What it stores                                                   |
| --- | ------------------- | ----------------- | ---------------------------------------------------------------- |
| 1   | **Database**        | Auto when serving | Confirms live Postgres; optional URL preference (`database_url`) |
| 2   | **AI review (LLM)** | Optional          | `llm_provider`, key / model / base URL — or `disabled`           |
| 3   | **GitHub App**      | Yes               | App ID, private key PEM, webhook secret, slug                    |
| 4   | **Admin**           | Yes               | First `users` row with role `admin`                              |

`GET /api/setup/status` returns booleans for each step plus `complete` when all required steps are true. Env vars already set at process start (`DATABASE_URL`, `OPENROUTER_API_KEY`, `GITHUB_APP_ID`, …) count as done. The database step is **true whenever the server is up** (Postgres connected at boot).

## Recommended path

1. `docker compose up` (starts Postgres 16 + Codasaurus).
2. Open `http://localhost:3000` → **Get started**.
3. Confirm **PostgreSQL** (Compose already wired `DATABASE_URL`) — see [database.md](database.md).
4. On LLM: use OpenRouter / Ollama, or **Skip for now** (Tier‑1 detectors still run).
5. **Create GitHub App** opens GitHub’s manifest form in a new tab; credentials save on callback.
6. Create the admin email/password (min 8 characters). There is no email reset yet — store the password safely.
7. On **Setup complete**: install the App on orgs/repos, then sign in.

## Routing behavior

- `/#/` and `/#/setup` redirect to login when setup is already complete.
- `/#/setup/complete` sends you back to the first incomplete step if anything is missing.
- After the first admin exists, mutating `/api/setup/*` requires an authenticated admin session.

## From source without Compose

Start Postgres yourself, then:

```bash
export DATABASE_URL="postgres://codasaurus:codasaurus@127.0.0.1:5432/codasaurus"
cargo run -- serve --port 3000
```
