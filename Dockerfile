# ── Stage 1: Build Svelte SPA ──────────────────────────────────
FROM node:20-alpine AS frontend
WORKDIR /app/svelte-dashboard
COPY svelte-dashboard/package*.json ./
RUN npm ci --ignore-scripts
COPY svelte-dashboard/ ./
RUN npm run build

# ── Stage 2: Build Rust binary ──────────────────────────────────
FROM rust:1.88-slim-bookworm AS backend
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
ENV CODASAURUS_SKIP_FRONTEND_BUILD=1
RUN mkdir -p src && echo "fn main() {}" > src/main.rs \
    && echo "" > src/lib.rs \
    && cargo build --release --locked 2>&1 | tail -1 \
    && rm -rf src

COPY . .
COPY --from=frontend /app/svelte-dashboard/dist/ svelte-dashboard/dist/
RUN cargo build --release --locked \
    && strip /app/target/release/codasaurus

# ── Stage 3: Runtime ────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r codasaurus --gid 65532 \
    && useradd -r -g codasaurus --uid 65532 --home-dir /data --create-home codasaurus \
    && mkdir -p /data /tmp/codasaurus \
    && chown -R 65532:65532 /data /tmp/codasaurus
WORKDIR /app
COPY --from=frontend /app/svelte-dashboard/dist/ /app/svelte-dashboard/dist/
COPY --from=backend /app/target/release/codasaurus /usr/local/bin/codasaurus
USER 65532:65532
ENV PORT=3000 \
    CODASAURUS_DATA_DIR=/data \
    HOME=/data
EXPOSE 3000
# Render sets PORT=10000; keep Docker HEALTHCHECK on the same port.
HEALTHCHECK --interval=30s --timeout=8s --start-period=90s --retries=5 \
  CMD ["sh", "-c", "codasaurus health --port ${PORT:-3000}"]
ENTRYPOINT ["codasaurus"]
CMD ["serve", "--host", "0.0.0.0"]
