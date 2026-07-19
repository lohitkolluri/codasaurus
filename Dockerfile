# Stage 1: Build Svelte SPA
FROM node:20-alpine AS frontend
WORKDIR /app/svelte-dashboard
COPY svelte-dashboard/package*.json ./
RUN npm ci
COPY svelte-dashboard/ ./
RUN npm run build

# Stage 2: Build Rust binary
# Keep the container compiler aligned with the MSRV of the locked dependency graph.
FROM rust:1.88-slim-bookworm AS backend
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY . .
# Copy pre-built Svelte assets so build.rs can skip npm
COPY --from=frontend /app/svelte-dashboard/dist/ svelte-dashboard/dist/
# Environment variable tells build.rs the frontend is already built
ENV CODASAURUS_SKIP_FRONTEND_BUILD=1
RUN cargo build --release --locked

# Stage 3: Runtime
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN groupadd -r codasaurus --gid 65532 && useradd -r -g codasaurus --uid 65532 codasaurus
RUN mkdir -p /data && chown -R 65532:65532 /data
WORKDIR /app
COPY --from=frontend /app/svelte-dashboard/dist/ /app/svelte-dashboard/dist/
COPY --from=backend /app/target/release/codasaurus /usr/local/bin/codasaurus
USER codasaurus
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=3s --retries=3 \
  CMD codasaurus health || exit 1
ENTRYPOINT ["codasaurus"]
CMD ["serve", "--host", "0.0.0.0", "--port", "3000"]
