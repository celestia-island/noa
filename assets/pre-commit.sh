#!/usr/bin/env bash
# noa-managed pre-commit hook: blocks commits that leak secrets or break the build.
# Managed by `noa hook install`. To remove, delete this file or re-run install --force.
# Checks performed (in Rust, via `noa hook pre-commit`):
#   1. Secret/credential scan of staged files (AWS/GitHub/npm/Slack/OpenAI keys, PEM private keys).
#   2. `cargo check --workspace` when the repo root contains a Cargo.toml (Rust projects only).
# Staged data is acquired from evernight IPC (host-side git) first, then local
# git; if neither source is reachable the gate silently passes (exit 0) so a
# data-source outage never blocks a commit. Real findings still abort.
# Unlike the commit-msg hook, this hook DOES block the commit on a finding.
#
# Escape valve: NOA_SKIP_HOOKS=1 git commit ...   bypasses all checks.
# Data-source tuning: EVERNIGHT_SOCK, NOA_EVERNIGHT_TIMEOUT_SECS,
#   NOA_HOST_REPO, NOA_LOCAL_REPO, NOA_HOOK_DATA_SOURCE (auto|evernight|local).

set -u

NOA_BIN="@NOA_BIN@"

if [ -n "${NOA_SKIP_HOOKS:-}" ]; then
    echo "[noa pre-commit] NOA_SKIP_HOOKS set; skipping secret/cargo checks." >&2
    exit 0
fi

if ! command -v "$NOA_BIN" >/dev/null 2>&1; then
    echo "[noa pre-commit] noa binary not found at '$NOA_BIN'; skipping checks." >&2
    exit 0
fi

"$NOA_BIN" hook pre-commit
exit $?
