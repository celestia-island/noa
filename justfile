# noa justfile

set shell := ["bash", "-c"]
set windows-shell := ["bash.exe", "-c"]
set unstable
set lists

# Shared celestia-devtools recipes — NOT in git. This justfile references shared
# variables, so the import is REQUIRED. Bootstrap once: celestia-devtools init
# (or `just fetch` if already staged). Refresh after upgrades.
import? "./.just/git-bash-interop.just"
import? "./.just/celestia-devtools.just"

# Stage shared celestia-devtools recipes into .just/ (gitignored).
# Source order: explicit URL arg → local pip bundle (offline) → GitHub raw.
# curl honors HTTP_PROXY/HTTPS_PROXY/ALL_PROXY env vars automatically.
[script('bash')]
fetch URL='':
    #!/usr/bin/env bash
    set -euo pipefail
    out=.just/celestia-devtools.just
    mkdir -p .just
    if [ -n "{{URL}}" ]; then
      echo "[fetch] {{URL}} -> $out"
      curl -fsSL "{{URL}}" -o "$out"
    elif command -v celestia-devtools >/dev/null 2>&1; then
      src=$(celestia-devtools include-path)
      echo "[fetch] local bundle ($src) -> $out"
      cp "$src" "$out"
    else
      echo "[fetch] github raw -> $out"
      curl -fsSL "https://raw.githubusercontent.com/celestia-island/celestia-devtools/dev/src/celestia_devtools/common.just" -o "$out"
    fi
    echo "[fetch] wrote $out"

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
