# noa justfile

set unstable
set lists
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

import "../just-common/build.just"

# Initialization

init:
    @echo "Initializing development environment..."
    cargo fetch
    @echo "Initialization complete!"

# Build

# Build noa. Release by default; `--dev` for debug, `--clean` to clean first.
build *FLAGS='':
    just _build ":" "cargo build" "cargo build --release" {{FLAGS}}

check:
    cargo check --workspace

clean:
    cargo clean

# Format & Lint

fmt:
    {{python_cmd}} scripts/utils/format_markdown.py .
    {{python_cmd}} scripts/utils/enforce_use_groups.py
    cargo fmt --all

fmt-check:
    {{python_cmd}} scripts/utils/format_markdown.py . --check
    {{python_cmd}} scripts/utils/enforce_use_groups.py --test
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
