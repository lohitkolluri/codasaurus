# Build Caddy with the DuckDNS DNS challenge module for automatic HTTPS.
# This allows Codasaurus to run behind a DuckDNS domain with Let's Encrypt.
FROM caddy:builder AS builder
RUN xcaddy build --with github.com/caddy-dns/duckdns

FROM caddy:alpine
COPY --from=builder /usr/bin/caddy /usr/bin/caddy
