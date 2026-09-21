# noa justfile

set shell := ["bash", "-c"]
# Windows: PowerShell (the 5.1 floor ships with every Windows; pwsh 7 is
# NOT assumed). Linewise recipes must stay PS-5.1-safe: no `&&` chains,
# `cd X; cmd` instead of `cd X && cmd`. Bash-only recipes use
# [script('bash')] and need Git Bash (or WSL) when actually run.
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command", "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $PSDefaultParameterValues['*:Encoding']='utf8';"]
set unstable
set lists

# Repo definitions override the shared template's (imported above).
set allow-duplicate-recipes
set allow-duplicate-variables

# Local fallbacks for the shared template's tool resolution — byte-identical
# semantics, so a fresh clone (no gitignored .just/ staging yet) parses and
# runs the same; when the staged template is present it re-defines the same
# values and allow-duplicate-variables lets either order win.
python_cmd := if os_family() == "windows" {
    if which("python") != "" { "python" } else { "python3" }
} else {
    if which("python3") != "" { "python3" } else { "python" }
}

# Shared celestia-devtools recipes — NOT in git. This justfile references shared
# variables, so the import is REQUIRED. Bootstrap once: celestia-devtools init
# (or `just fetch` if already staged). Refresh after upgrades.
import? "./.just/git-bash-interop.just"
import? "./.just/celestia-devtools.just"

# Stage shared celestia-devtools recipes into .just/ (gitignored).
# Source order: explicit URL arg → local pip bundle (offline) → GitHub raw.
# curl honors HTTP_PROXY/HTTPS_PROXY/ALL_PROXY env vars automatically.
fetch URL='':
    {{ if os_family() == "windows" { "python" } else { "python3" } }} -c "import os; os.makedirs('.just', exist_ok=True)"
    {{ if URL != "" { "curl -fsSL " + URL + " -o .just/celestia-devtools.just" } else if which("celestia-devtools") != "" { "celestia-devtools fetch-just" } else { "curl -fsSL https://raw.githubusercontent.com/celestia-island/celestia-devtools/dev/src/celestia_devtools/common.just -o .just/celestia-devtools.just" } }}
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
    just fmt-toml
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
