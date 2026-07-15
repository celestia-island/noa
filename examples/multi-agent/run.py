#!/usr/bin/env python3
"""Example 2: Multi-agent sequential workspace creation + snapshot.

Note: redb uses an exclusive file lock, so concurrent CLI processes sharing
the same database is not supported. In production, agents share a single
long-lived process or use the noa-server HTTP API for concurrency.
This example demonstrates sequential multi-agent workspace creation.
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent / "scripts" / "tests"))
from utils import noa, init_repo, log

REPO = Path("/tmp/noa-example-multi-agent")

NUM_AGENTS = 10


def main() -> None:
    print("=== Example 2: Multi-Agent Workflow ===")

<<<<<<< HEAD
    log(f"Initializing repo")
=======
    log("Initializing repo")
>>>>>>> origin/dev
    init_repo(REPO)

    log(f"Creating {NUM_AGENTS} agent workspaces sequentially")
    for i in range(NUM_AGENTS):
        ws_name = f"agent-{i:03d}"
        noa("workspace", "create", ws_name, "--agent", f"bot-{i}", cwd=REPO)
        noa("workspace", "switch", ws_name, cwd=REPO)
        noa("snapshot", "create", "-m", f"snapshot from {ws_name}", "-a", ws_name, cwd=REPO)
        noa("workspace", "switch", "default", cwd=REPO)
        log(f"  Completed: {ws_name}")

    log("Verifying workspaces")
    r = noa("workspace", "list", cwd=REPO)
    for i in range(NUM_AGENTS):
        ws_name = f"agent-{i:03d}"
        assert ws_name in r.stdout, f"workspace {ws_name} not found"
    log(f"Found all {NUM_AGENTS} workspaces")

    log("Verifying snapshots (all workspaces)")
    r = noa("log", "--workspace", "agent-000", "--limit", "50", cwd=REPO)
    snapshot_count = r.stdout.count("noa_")
    log(f"Found {snapshot_count} snapshots in agent-000")
<<<<<<< HEAD
    assert snapshot_count >= 1, f"expected at least 1 snapshot"
=======
    assert snapshot_count >= 1, "expected at least 1 snapshot"
>>>>>>> origin/dev

    r = noa("snapshot", "list", cwd=REPO)
    total = r.stdout.count("noa_")
    log(f"Total snapshots across all workspaces: {total}")
    assert total == NUM_AGENTS, f"expected {NUM_AGENTS} snapshots, got {total}"

    log("Listing workspaces:")
    r = noa("workspace", "list", cwd=REPO)
    for line in r.stdout.strip().splitlines():
        log(f"  {line}")

    print(f"\n=== Example 2 PASSED ({NUM_AGENTS} agents) ===\n")


if __name__ == "__main__":
    main()
