.PHONY: all dev build frontend frontend-build check test clean docker

# Default target
all: build

# Development — build frontend first, then cargo
dev: frontend
	cargo build

# Build frontend (install deps if needed)
frontend:
	cd svelte-dashboard && npm install && npm run build

# Full release build
build:
	cargo build --release

# Quick check
check:
	cargo check

# Run tests
test:
	cargo test

# Clean
clean:
	cargo clean
	rm -rf svelte-dashboard/node_modules svelte-dashboard/dist

# Docker build
docker:
	docker build -t codasaurus:latest .

# Development Docker (with hot-reload — mounts source)
docker-dev:
	docker compose up --build
