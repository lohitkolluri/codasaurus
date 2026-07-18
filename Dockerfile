# syntax=docker/dockerfile:1
# Multi-stage build with cargo-chef dependency caching + distroless runtime

# Stage 1: Cargo chef with pinned Rust toolchain
FROM rust:1.85-slim-bookworm AS chef
RUN cargo install cargo-chef --locked && rm -rf /usr/local/cargo/registry/cache/*
WORKDIR /app

# Stage 2: Generate dependency recipe from metadata only
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build dependencies + binary
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/target,sharing=locked \
    cargo build --release --bin codasaurus && \
    cp /target/release/codasaurus /usr/local/bin/codasaurus

# Stage 4: Test gate
FROM builder AS test
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/target,sharing=locked \
    cargo test --release

# Stage 5: Distroless runtime (no apt, no shell)
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime

COPY --from=builder /usr/local/bin/codasaurus /codasaurus

ENV PORT=3000
EXPOSE 3000
USER 65532:65532
ENTRYPOINT ["/codasaurus"]
CMD ["serve"]
