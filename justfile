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

# NOTE: vein's `sqlite`/`postgres` adapter features are mutually exclusive
# (compile_error! guards), so we build with the default feature set rather
# than `--all-features`. Use `just check-postgres` for the postgres variant.
check:
    cargo check --workspace --all-targets

check-postgres:
    cargo check -p vein --no-default-features --features postgres,tls --all-targets

clippy:
    cargo clippy --workspace --all-targets

doc:
    cargo doc --no-deps --workspace

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
