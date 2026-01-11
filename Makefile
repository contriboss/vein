# Default configuration file; override with `make CONFIG=custom.toml run`
CONFIG ?= vein.toml
ADMIN_BIND ?= 127.0.0.1
ADMIN_PORT ?= 9400

.PHONY: help run admin dev stats cache-refresh check fmt clippy test build release pg pg-release pg-run pg-admin docker-build docker-up docker-down docker-logs

help:
	@echo "Available targets:"
	@echo "  run            Start the Vein proxy (SQLite)"
	@echo "  admin          Start the admin dashboard (SQLite)"
	@echo "  stats          Show cache statistics"
	@echo "  cache-refresh  Refresh the hot cache"
	@echo "  build          Build debug binaries (SQLite)"
	@echo "  release        Build release binaries (SQLite)"
	@echo ""
	@echo "PostgreSQL targets:"
	@echo "  pg             Build debug binaries (PostgreSQL)"
	@echo "  pg-release     Build release binaries (PostgreSQL)"
	@echo "  pg-run         Run proxy (PostgreSQL)"
	@echo "  pg-admin       Run admin (PostgreSQL)"
	@echo ""
	@echo "Docker targets:"
	@echo "  docker-build   Build Docker image"
	@echo "  docker-up      Start containers"
	@echo "  docker-down    Stop containers"
	@echo "  docker-logs    View vein logs"
	@echo ""
	@echo "Dev targets:"
	@echo "  check          Type-check workspace"
	@echo "  fmt            Format workspace"
	@echo "  clippy         Lint workspace"
	@echo "  test           Run tests"

run:
	cargo run -- serve --config $(CONFIG)

dev:
	@if ! command -v cargo-watch >/dev/null 2>&1; then \
		echo "cargo-watch is required for 'make dev' (install with 'cargo install cargo-watch')."; \
		exit 1; \
	fi
	cargo watch -x "run -- serve --config $(CONFIG)"

stats:
	cargo run -- stats --config $(CONFIG)

cache-refresh:
	cargo run -- cache refresh --config $(CONFIG)

admin:
	cargo run -p vein-admin -- --config $(CONFIG) --bind $(ADMIN_BIND) --port $(ADMIN_PORT)

check:
	cargo check

fmt:
	cargo fmt

clippy:
	cargo clippy

test:
	cargo test

build:
	cargo build

release:
	cargo build --release

# PostgreSQL builds
pg:
	cargo build --no-default-features --features tls,postgres

pg-release:
	cargo build --release --no-default-features --features tls,postgres

pg-run:
	cargo run --no-default-features --features tls,postgres -- serve --config $(CONFIG)

pg-admin:
	cargo run -p vein-admin --no-default-features --features postgres -- --config $(CONFIG) --bind $(ADMIN_BIND) --port $(ADMIN_PORT)

# Docker
docker-build:
	docker compose build

docker-up:
	docker compose up -d

docker-down:
	docker compose down

docker-logs:
	docker compose logs -f vein
