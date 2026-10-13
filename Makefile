# Default configuration file; override with `make CONFIG=custom.toml run`
CONFIG ?= vein.toml

.PHONY: help run dev stats check fmt clippy test build release

help:
	@echo "Available targets:"
	@echo "  run            Start the Vein proxy and admin dashboard (cargo run -- serve)"
	@echo "  stats          Show cache statistics"
	@echo "  check          Type-check the workspace"
	@echo "  fmt            Format the workspace"
	@echo "  clippy         Lint with cargo clippy"
	@echo "  test           Run the workspace tests"
	@echo "  build          Build debug binaries"
	@echo "  release        Build release binaries"

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
