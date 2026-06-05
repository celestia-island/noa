# noa justfile

set unstable
set shell := ["bash", "-c"]
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

python_cmd := if os_family() == "windows" {
    "python"
} else if which("python3") != "" {
    "python3"
} else {
    "python"
}

default:
    @just --list

# Initialization

init:
    @echo "Initializing development environment..."
    cargo fetch
    @echo "Initialization complete!"

# Build

build:
    cargo build --release

build-dev:
    cargo build

check:
    cargo check --workspace

clean:
    cargo clean

# Format & Lint

fmt:
    {{python_cmd}} scripts/utils/format_markdown.py .
    cargo fmt --all

fmt-check:
    {{python_cmd}} scripts/utils/format_markdown.py . --check
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --lib --bins -- -D warnings

# Test

test:
    cargo test --all-targets --all-features --workspace --no-fail-fast

test-integration:
    cargo test --test '*' --all-features --workspace --no-fail-fast

# CI

ci: fmt-check clippy check test

# Run

run *ARGS:
    cargo run -- {{ARGS}}
