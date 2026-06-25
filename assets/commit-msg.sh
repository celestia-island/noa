#!/usr/bin/env bash
# noa-managed commit-msg hook: appends AI agent co-author trailers.
# Managed by `noa hook install`. To remove, delete this file or re-run install --force.
# This hook NEVER blocks a commit: on any resolver failure it exits 0 silently.

set -u

COMMIT_MSG_FILE="$1"
NOA_BIN="@NOA_BIN@"

if [ -z "${NOA_COAUTHOR_DISABLE:-}" ] && command -v "$NOA_BIN" >/dev/null 2>&1; then
    BLOCK="$("$NOA_BIN" co-author resolve 2>/dev/null)" || BLOCK=""
    if [ -n "$BLOCK" ]; then
        EXISTING="$(cat "$COMMIT_MSG_FILE" 2>/dev/null)" || EXISTING=""
        if ! printf '%s' "$EXISTING" | grep -q 'Co-authored-by:'; then
            if [ -n "$EXISTING" ] && [ "$(printf '%s' "$EXISTING" | tail -c1)" != "" ]; then
                printf '\n\n' >>"$COMMIT_MSG_FILE"
            elif [ -n "$EXISTING" ]; then
                printf '\n' >>"$COMMIT_MSG_FILE"
            fi
            printf '%s\n' "$BLOCK" >>"$COMMIT_MSG_FILE"
        fi
    fi
fi

exit 0
