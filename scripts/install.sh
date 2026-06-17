#!/usr/bin/env bash
# noa installer script for Linux & macOS
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/celestia-island/noa/dev/scripts/install.sh | bash
#   or
#   bash install.sh [--from-source]
#
# Options:
#   --from-source    Build from source (requires Rust 1.85+)
#   --version X.Y.Z  Install a specific version (default: latest)
#   --prefix PATH    Installation directory (default: ~/.local/bin)

set -euo pipefail

# --- Defaults ---
DEFAULT_PREFIX="$HOME/.local/bin"
GITHUB_REPO="celestia-island/noa"
BINARY="noa"
SERVER_BINARY="noa-server"
INSTALL_VERSION="${NOA_VERSION:-latest}"
FROM_SOURCE=false
PREFIX=""

# --- Parse args ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --from-source) FROM_SOURCE=true; shift ;;
        --version) INSTALL_VERSION="$2"; shift 2 ;;
        --prefix) PREFIX="$2"; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--from-source] [--version X.Y.Z] [--prefix PATH]"
            echo ""
            echo "Install noa — AI-native distributed version control system"
            echo ""
            echo "Options:"
            echo "  --from-source    Build from source (requires Rust 1.85+)"
            echo "  --version X.Y.Z  Install a specific version"
            echo "  --prefix PATH    Installation directory (default: ~/.local/bin)"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

PREFIX="${PREFIX:-$DEFAULT_PREFIX}"

# --- Detect platform ---
detect_platform() {
    local os arch
    case "$(uname -s)" in
        Linux)  os="linux" ;;
        Darwin) os="macos" ;;
        *)
            echo "Unsupported OS: $(uname -s). Use --from-source to build." >&2
            exit 1
            ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64)  arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *)
            echo "Unsupported architecture: $(uname -m). Use --from-source to build." >&2
            exit 1
            ;;
    esac
    echo "${os}-${arch}"
}

# --- Install from prebuilt binary ---
install_from_release() {
    local platform version asset_url tmpdir archive
    platform="$(detect_platform)"

    if [[ "$INSTALL_VERSION" == "latest" ]]; then
        echo "→ Fetching latest release version..."
        version=$(curl -fsSL "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" \
            | grep -o '"tag_name": "[^"]*"' \
            | head -1 \
            | cut -d'"' -f4)
        if [[ -z "$version" ]]; then
            echo "Failed to fetch latest version. Falling back to --from-source." >&2
            install_from_source
            return
        fi
    else
        version="v${INSTALL_VERSION#v}"
    fi
    echo "→ Installing noa ${version}"

    case "$platform" in
        linux-x86_64)    asset_name="noa-linux-x86_64.tar.gz" ;;
        linux-aarch64)   asset_name="noa-linux-aarch64.tar.gz" ;;
        macos-x86_64)    asset_name="noa-macos-x86_64.tar.gz" ;;
        macos-aarch64)   asset_name="noa-macos-aarch64.tar.gz" ;;
    esac

    asset_url="https://github.com/${GITHUB_REPO}/releases/download/${version}/${asset_name}"
    tmpdir="$(mktemp -d)"
    archive="${tmpdir}/${asset_name}"

    echo "→ Downloading ${asset_name}..."
    if ! curl -fsSL -o "$archive" "$asset_url"; then
        echo "Prebuilt binary not found for ${platform}. Falling back to --from-source." >&2
        rm -rf "$tmpdir"
        install_from_source
        return
    fi

    echo "→ Extracting..."
    tar -xzf "$archive" -C "$tmpdir"

    mkdir -p "$PREFIX"
    for bin in "${BINARY}" "${SERVER_BINARY}"; do
        if [[ -f "${tmpdir}/${bin}" ]]; then
            install -m 755 "${tmpdir}/${bin}" "${PREFIX}/${bin}"
            echo "✓ Installed ${PREFIX}/${bin}"
        fi
    done

    rm -rf "$tmpdir"
}

# --- Install from source ---
install_from_source() {
    echo "→ Building from source (requires Rust 1.85+)..."

    if ! command -v cargo &>/dev/null; then
        echo "Rust is not installed. Install it first: https://rustup.rs" >&2
        exit 1
    fi

    local rust_ver
    rust_ver=$(rustc --version | grep -oE '[0-9]+\.[0-9]+')
    if [[ -z "$rust_ver" ]]; then
        echo "Could not detect Rust version." >&2
        exit 1
    fi
    if [[ "$(printf '%s\n' "1.85" "$rust_ver" | sort -t. -k1,1n -k2,2n | head -1)" != "1.85" ]]; then
        echo "Rust >= 1.85 required. Found: $(rustc --version)" >&2
        echo "Update with: rustup update" >&2
        exit 1
    fi

    local tmpdir ref
    tmpdir="$(mktemp -d)"
    ref="${INSTALL_VERSION}"
    [[ "$ref" == "latest" ]] && ref="dev"

    echo "→ Cloning repository (branch: ${ref})..."
    git clone --depth 1 --branch "$ref" "https://github.com/${GITHUB_REPO}.git" "$tmpdir" 2>/dev/null || {
        echo "Branch '${ref}' not found. Using default branch..."
        git clone --depth 1 "https://github.com/${GITHUB_REPO}.git" "$tmpdir"
    }

    echo "→ Building (release)..."
    cargo build --release --manifest-path "${tmpdir}/Cargo.toml" || {
        echo "Build failed." >&2
        rm -rf "$tmpdir"
        exit 1
    }

    mkdir -p "$PREFIX"
    for bin in "${BINARY}" "${SERVER_BINARY}"; do
        if [[ -f "${tmpdir}/target/release/${bin}" ]]; then
            install -m 755 "${tmpdir}/target/release/${bin}" "${PREFIX}/${bin}"
            echo "✓ Installed ${PREFIX}/${bin}"
        fi
    done

    rm -rf "$tmpdir"
}

# --- Verify PATH ---
verify_path() {
    if [[ ":$PATH:" != *":$PREFIX:"* ]]; then
        local shell_rc
        case "$(basename "$SHELL")" in
            zsh)  shell_rc="$HOME/.zshrc" ;;
            bash) shell_rc="$HOME/.bashrc" ;;
            fish) shell_rc="$HOME/.config/fish/config.fish" ;;
            *)    shell_rc="$HOME/.profile" ;;
        esac
        echo ""
        echo "⚠  ${PREFIX} is not in your PATH."
        echo "   Add this to ${shell_rc}:"
        echo ""
        echo "   export PATH=\"${PREFIX}:\$PATH\""
        echo ""
    fi
}

# --- Main ---
echo "╔══════════════════════════════════╗"
echo "║  noa installer  (v0.3.0)         ║"
echo "╚══════════════════════════════════╝"
echo ""

if $FROM_SOURCE; then
    install_from_source
else
    install_from_release
fi

echo ""

if command -v "${BINARY}" &>/dev/null; then
    echo "✓ noa installed successfully!"
    noa --version 2>/dev/null || true
else
    verify_path
    # Try with full path
    if [[ -x "${PREFIX}/${BINARY}" ]]; then
        echo "Run: ${PREFIX}/${BINARY} --help"
    fi
fi
