# Contributing

Thanks for helping improve Codasaurus.

## Prerequisites

- Rust **1.88+** (see `rust-toolchain.toml`)
- Node **20+** (dashboard)
- PostgreSQL **16+** (local or Docker)

## Setup

```bash
git clone https://github.com/lohitkolluri/codasaurus.git
cd codasaurus
cp .env.example .env   # optional; compose works with defaults
docker compose up -d postgres
export DATABASE_URL="postgres://codasaurus:codasaurus@127.0.0.1:5432/codasaurus"
cargo run -- serve --port 3000
```

Dashboard (hot reload):

```bash
cd svelte-dashboard
npm ci
npm run dev
```

## Checks before a PR

```bash
cargo fmt --check
cargo clippy -- -D warnings
CODASAURUS_SKIP_FRONTEND_BUILD=1 cargo test
cd svelte-dashboard && npm ci && npm run build
```

CI runs the same gates on every push.

## Scope

- Prefer small, focused PRs
- Match existing style; avoid drive-by refactors
- Update docs when behavior or Settings paths change
- Do not commit secrets, `.env`, or research dumps

## License

By contributing, you agree your changes are licensed under **AGPL-3.0-or-later** (see [LICENSE](LICENSE)).
