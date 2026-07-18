# syntax=docker/dockerfile:1
# =============================================================================
# Codasaurus — Production Docker Image
# Multi-stage build with cargo-chef dependency caching + minimal runtime
# =============================================================================

# Stage 1: Cargo chef image with Rust toolchain (pre-built, no compile wait)
FROM lukemathwalker/cargo-chef:latest-rust-1.88 AS chef
WORKDIR /app

# Stage 2: Generate dependency recipe (only Cargo.toml/lock + src metadata)
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build dependencies + application binary
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

# Build dependencies only (cached unless Cargo.toml/Cargo.lock changes)
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json

# Copy source and build the full binary
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release --bin codasaurus && \
    cp /app/target/release/codasaurus /usr/local/bin/codasaurus

# Stage 4: Run tests (optional — gates runtime on passing tests)
FROM builder AS test
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo test --release --locked

# Stage 5: Minimal production runtime
FROM debian:bookworm-slim AS runtime

# ca-certificates for HTTPS (GitHub API, OpenRouter), curl for HEALTHCHECK
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# Non-root user for security
RUN addgroup --system --gid 1001 appgroup && \
    adduser --system --uid 1001 --gid 1001 --no-create-home --disabled-login appuser

COPY --from=builder /usr/local/bin/codasaurus /usr/local/bin/codasaurus

ENV PORT=3000
EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -sf http://localhost:$PORT/health || exit 1

USER appuser:appgroup
ENTRYPOINT ["/usr/local/bin/codasaurus"]
CMD ["serve"]
