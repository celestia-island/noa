#!/usr/bin/env bash
# Quick local deployment for noa-server (P6#C2).
#
# Builds/installs the noa binaries and starts noa-server as a systemd unit
# (preferred) or a docker-compose service (--docker).
#
# Usage:
#   scripts/deploy.sh [--docker] [--port 3000] [--host 127.0.0.1]
#
# The API token is read from NOA_API_TOKEN (env) or generated once and stored
# in /etc/noa-server.env. Print it with: cat /etc/noa-server.env
set -euo pipefail

PORT="${PORT:-3000}"
HOST="${HOST:-127.0.0.1}"
MODE="systemd"
while [ $# -gt 0 ]; do
    case "$1" in
        --docker) MODE="docker" ;;
        --port) PORT="$2"; shift ;;
        --host) HOST="$2"; shift ;;
        *) echo "unknown option: $1" >&2; exit 1 ;;
    esac
    shift
done

PREFIX="${PREFIX:-/usr/local}"
DB_DIR="${DB_DIR:-/var/lib/noa-server}"
ENV_FILE="${ENV_FILE:-/etc/noa-server.env}"

if [ "$(id -u)" -ne 0 ] && [ "$MODE" = "systemd" ]; then
    echo "systemd mode requires root (or run with --docker)" >&2
    exit 1
fi

# ── 1. Ensure the API token ──────────────────────────────────────────────
if [ -z "${NOA_API_TOKEN:-}" ]; then
    if [ -f "$ENV_FILE" ]; then
        NOA_API_TOKEN="$(grep '^NOA_API_TOKEN=' "$ENV_FILE" | cut -d= -f2-)"
    fi
fi
if [ -z "${NOA_API_TOKEN:-}" ]; then
    NOA_API_TOKEN="$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')"
    echo "generated NOA_API_TOKEN=$NOA_API_TOKEN"
fi

# ── 2. Build (if source tree available) or download a release ────────────
if [ -f Cargo.toml ]; then
    echo "building noa from source..."
    if command -v cargo >/dev/null 2>&1; then
        cargo build --release --bin noa --bin noa-server
        BIN_DIR="$(pwd)/target/release"
    else
        echo "cargo not found; falling back to release download" >&2
        BIN_DIR=""
    fi
else
    BIN_DIR=""
fi
if [ -z "${BIN_DIR:-}" ]; then
    echo "downloading noa release..."
    bash scripts/install.sh
    BIN_DIR="$HOME/.local/bin"
fi

# ── 3. Install binaries ──────────────────────────────────────────────────
if [ "$MODE" = "systemd" ]; then
    install -d "$PREFIX/bin"
    install -m 0755 "$BIN_DIR/noa" "$PREFIX/bin/noa"
    install -m 0755 "$BIN_DIR/noa-server" "$PREFIX/bin/noa-server"
    install -d "$DB_DIR"
    install -m 0600 /dev/null "$ENV_FILE"
    if ! grep -q '^NOA_API_TOKEN=' "$ENV_FILE"; then
        echo "NOA_API_TOKEN=$NOA_API_TOKEN" >> "$ENV_FILE"
    fi

    # ── 4a. systemd unit ──────────────────────────────────────────────
    install -m 0644 packaging/noa-server.service /etc/systemd/system/noa-server.service
    sed -i "s|--host 127.0.0.1 --port 3000|--host $HOST --port $PORT|" \
        /etc/systemd/system/noa-server.service
    systemctl daemon-reload
    systemctl enable --now noa-server.service
    echo "noa-server started via systemd on $HOST:$PORT (token in $ENV_FILE)"
else
    # ── 4b. docker-compose ────────────────────────────────────────────
    if ! command -v docker >/dev/null 2>&1; then
        echo "docker not found" >&2
        exit 1
    fi
    cat > docker-compose.noa.yml <<EOF
services:
  noa-server:
    image: rust:bookworm
    command: sh -c "cargo install --path /noa --bin noa-server --root /usr/local && NOA_API_TOKEN=$NOA_API_TOKEN /usr/local/bin/noa-server --host 0.0.0.0 --port $PORT --db-path /data/noa-server.redb"
    volumes:
      - .:/noa
      - noa-data:/data
    ports:
      - "${HOST}:${PORT}:${PORT}"
    restart: unless-stopped

volumes:
  noa-data:
EOF
    docker compose -f docker-compose.noa.yml up -d --build
    echo "noa-server started via docker-compose on $HOST:$PORT (token: $NOA_API_TOKEN)"
fi

echo "verify: curl -s -H \"Authorization: Bearer $NOA_API_TOKEN\" http://$HOST:$PORT/api/v1/refs"
