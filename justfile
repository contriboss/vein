export RUSTFLAGS := "-D warnings"
export RUSTDOCFLAGS := "-D warnings"

# Default config file; override with `just CONFIG=custom.toml run`.
CONFIG := "vein.toml"

_default:
    @just --list

fmt *ARGS:
    cargo fmt --all {{ARGS}}

fmt-check *ARGS:
    cargo fmt --all --check {{ARGS}}

check:
    cargo check --workspace --all-targets

clippy:
    cargo clippy --workspace --all-targets

doc:
    cargo doc --no-deps --workspace

# Postgres URL the test suite may create schemas in (one per test).
export VEIN_TEST_DATABASE_URL := env("VEIN_TEST_DATABASE_URL", "postgres://vein:vein@127.0.0.1:5432/vein_test")

test *ARGS:
    cargo test --workspace {{ARGS}}

deny:
    @command -v cargo-deny >/dev/null || cargo install cargo-deny --locked
    cargo deny --workspace check

# Quick quality: format, type-check, lint, docs. No tests.
qq: fmt-check check clippy doc

# Full quality assurance: qq plus tests.
qa: qq test

# Run the proxy.
run:
    cargo run -- serve --config {{CONFIG}}

clean:
    cargo clean
