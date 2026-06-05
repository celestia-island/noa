#!/usr/bin/env python3
"""Example 1: Basic workflow — init, workspace, snapshot, log, diff."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent / "scripts" / "tests"))
from utils import noa, init_repo, log

REPO = Path("/tmp/noa-example-basic")


def main() -> None:
    print("=== Example 1: Basic Workflow ===")

    log("Initializing repo")
    init_repo(REPO)

    log("Checking status")
    r = noa("status", cwd=REPO)
    assert "default" in r.stdout, f"expected 'default' in status, got: {r.stdout}"
    log(f"Status: {r.stdout.strip()}")

    log("Listing initial workspaces")
    r = noa("workspace", "list", cwd=REPO)
    log(f"Workspaces: {r.stdout.strip()}")

    log("Creating workspace 'feature-1'")
    noa("workspace", "create", "feature-1", cwd=REPO)

    log("Switching to 'feature-1'")
    noa("workspace", "switch", "feature-1", cwd=REPO)

    r = noa("status", cwd=REPO)
    assert "feature-1" in r.stdout, f"expected 'feature-1' in status"
    log(f"Status: {r.stdout.strip()}")

    log("Creating snapshot with message")
    noa("snapshot", "create", "-m", "initial empty snapshot", cwd=REPO)

    r = noa("snapshot", "list", cwd=REPO)
    assert "initial empty snapshot" in r.stdout, f"snapshot not found in list"
    log(f"Snapshots: {r.stdout.strip()}")

    log("Creating another snapshot")
    noa("snapshot", "create", "-m", "second snapshot", "-a", "agent-1", cwd=REPO)

    r = noa("log", cwd=REPO)
    assert "agent-1" in r.stdout, f"author not found in log"
    log(f"Log: {r.stdout.strip()}")

    log("Adding remote")
    noa("remote", "add", "origin", "https://github.com/example/test.git", cwd=REPO)

    r = noa("remote", "list", cwd=REPO)
    assert "origin" in r.stdout, f"remote not listed"
    log(f"Remotes: {r.stdout.strip()}")

    log("Switching back to default")
    noa("workspace", "switch", "default", cwd=REPO)

    log("Deleting 'feature-1'")
    noa("workspace", "delete", "feature-1", cwd=REPO)

    r = noa("workspace", "list", cwd=REPO)
    assert "feature-1" not in r.stdout, f"workspace not deleted"
    log(f"Final workspaces: {r.stdout.strip()}")

    print("\n=== Example 1 PASSED ===\n")


if __name__ == "__main__":
    main()
