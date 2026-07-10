# noa justfile

set unstable
set lists
set windows-shell := ["C:/Program Files/Git/bin/bash.exe", "-c"]
set shell := ["bash", "-c"]
set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $PSDefaultParameterValues['*:Encoding'] = 'utf8';"]
set lists

import "./celestia-devtools.just"

default:
    @just --list

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
    cargo clippy --workspace --lib --bins -- -D warnings
    {{ python_cmd }} scripts/utils/enforce_use_groups.py
    cargo fmt --all

fmt-check:
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
