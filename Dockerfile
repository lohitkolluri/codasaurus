# syntax=docker/dockerfile:1
# Multi-stage build with cargo-chef dependency caching + distroless runtime

# Stage 1: Cargo chef with pinned Rust toolchain
FROM rust:1.85-slim-bookworm AS chef
RUN cargo install cargo-chef --locked && rm -rf /usr/local/cargo/registry/cache/*
WORKDIR /app

# Stage 2: Generate dependency recipe from metadata only (no src/ — recipe only needs manifests)
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build dependencies + binary
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --locked --release --bin codasaurus && \
    cp /app/target/release/codasaurus /usr/local/bin/codasaurus

# Stage 4: Distroless runtime (no apt, no shell)
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime

LABEL org.opencontainers.image.source="https://github.com/lohitkolluri/codasaurus"
LABEL org.opencontainers.image.description="Static and AI-powered code review for AI-generated changes"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.version="${CODASAURUS_VERSION:-unknown}"

COPY --from=builder /usr/local/bin/codasaurus /codasaurus

ENV PORT=3000
ENV CODASAURUS_DATA_DIR=/data
EXPOSE 3000
USER 65532:65532
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD ["/codasaurus", "health", "--port", "3000"]
ENTRYPOINT ["/codasaurus"]
CMD ["serve"]
