# Codasaurus — GitHub App Setup

## 1. GitHub App Registration

1. Go to **Settings → Developer settings → GitHub Apps → New GitHub App**
2. Fill in:
   - **GitHub App name:** `codasaurus` (or your choice)
   - **Homepage URL:** `https://github.com/lohitkolluri/codasaurus`
   - **Webhook URL:** `https://your-deployment.com/webhook` (replace with your actual URL)
   - **Webhook secret:** generate a random string (e.g. `openssl rand -hex 32`)
3. **Permissions:**
   - Pull requests: **Read & Write**
   - Contents: **Read**
   - Issues: **Read & Write**
   - Metadata: **Read**
4. **Subscribe to events:**
   - Pull request
5. **Where can this app be installed:** Any account

## 2. Generate a Private Key

- Scroll to **Private keys** and click **Generate a private key**
- A `.pem` file will download — store it securely and never commit it

## 3. Installation

- After creating the app, go to the app's settings page
- Under **Install App**, click **Install** next to your account/organization
- Select the repositories you want the bot to monitor

## 4. Deployment

### Option A: Docker

```bash
docker-compose up
```

### Option B: Fly.io

```bash
fly deploy
```

### Option C: Railway / Render

Point the platform at the repo root, set the start command to your server binary, and configure the environment variables in the dashboard.

## 5. Environment Variables

| Variable | Required | Description |
|---|---|---|
| `GITHUB_APP_ID` | Yes | Numeric App ID from the GitHub App settings page |
| `GITHUB_APP_PRIVATE_KEY` | Yes | The full contents of the `.pem` file (or `-----BEGIN RSA PRIVATE KEY-----` block, with literal newlines) |
| `GITHUB_WEBHOOK_SECRET` | Yes | The webhook secret you set during registration |
| `OPENROUTER_API_KEY` | No | API key for OpenRouter (used for AI-powered review) |
| `ENVIRONMENT` | No | `dev` or `prod` — controls logging verbosity, etc. |

## 6. Verification

Once deployed, verify the server is running:

```bash
curl https://your-deployment.com/health
```

Expected response: `ok`

Then open a pull request on one of the installed repositories. The bot should receive the webhook event and post a review.
