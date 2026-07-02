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

_build always_pre dcmd rcmd *FLAGS='':
    #!/usr/bin/env bash
    set -euo pipefail
    profile=release
    for a in {{FLAGS}}; do
      case "$a" in
        --dev)   profile=dev ;;
        --clean) cargo clean ;;
      esac
    done
    [ "X{{always_pre}}" != "X:" ] && {{always_pre}}
    if [ "$profile" = dev ]; then {{dcmd}}; else {{rcmd}}; fi

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
