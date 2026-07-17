.PHONY: all build install check test clean lint release help

APP_NAME = codasaurus

all: check build

## Build the project in release mode
build:
	cargo build --release

## Build in debug mode (fast iteration)
build-dev:
	cargo build

## Install locally
install: build
	cp target/release/$(APP_NAME) /usr/local/bin/

## Run checks (clippy + fmt)
check:
	cargo clippy -- -D warnings
	cargo fmt --check

## Run tests
test:
	cargo test

## Run with staged changes (local dev)
run-staged:
	cargo run -- check --staged

## Run with JSON output
run-json:
	cargo run -- check --staged --json

## Run CI mode
run-ci:
	cargo run -- check --diff origin/main --ci

## Clean build artifacts
clean:
	cargo clean

## Run lints
lint: check

## Watch mode (experimental)
watch:
	cargo run -- watch

## Build for multiple platforms
release:
	cargo build --release
	@echo "Build complete: target/release/$(APP_NAME)"

## Cross-compile for all platforms (requires cross toolchains)
release-all:
	@echo "Building for multiple platforms..."
	cargo build --release
	@echo "Building for x86_64 macOS..."
	cargo build --release --target x86_64-apple-darwin 2>/dev/null || true
	@echo "Building for x86_64 Linux..."
	# cargo build --release --target x86_64-unknown-linux-gnu 2>/dev/null || true
	@echo "Building for ARM64 Linux..."
	# cargo build --release --target aarch64-unknown-linux-gnu 2>/dev/null || true

## Install pre-commit hook (optional for power users)
install-hook:
	@echo "Installing pre-commit hook..."
	@mkdir -p .git/hooks
	@echo '#!/bin/sh' > .git/hooks/pre-commit
	@echo 'codasaurus check --staged || exit 1' >> .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "Pre-commit hook installed."

## Show help
help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'
