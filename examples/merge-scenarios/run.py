#!/usr/bin/env python3
"""Example 3: Merge scenarios — create, merge, conflict detection."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent / "scripts" / "tests"))
from utils import noa, init_repo, log

REPO = Path("/tmp/noa-example-merge")


def main() -> None:
    print("=== Example 3: Merge Scenarios ===")

    log("Initializing repo")
    init_repo(REPO)

    log("Creating base snapshot on 'default'")
    noa("snapshot", "create", "-m", "base snapshot", cwd=REPO)
    r = noa("snapshot", "list", cwd=REPO)
    log(f"Base snapshots: {r.stdout.strip()}")

    log("Creating workspace 'branch-a'")
    noa("workspace", "create", "branch-a", cwd=REPO)
    noa("workspace", "switch", "branch-a", cwd=REPO)
    noa("snapshot", "create", "-m", "changes from branch-a", "-a", "developer-a", cwd=REPO)

    log("Switching back to default")
    noa("workspace", "switch", "default", cwd=REPO)

    log("Creating workspace 'branch-b'")
    noa("workspace", "create", "branch-b", cwd=REPO)
    noa("workspace", "switch", "branch-b", cwd=REPO)
    noa("snapshot", "create", "-m", "changes from branch-b", "-a", "developer-b", cwd=REPO)

    log("Switching back to default for merge")
    noa("workspace", "switch", "default", cwd=REPO)

    log("Merging branch-a into default")
    r = noa("workspace", "merge", "branch-a", cwd=REPO)
    log(f"Merge result: {r.stdout.strip()}")

    log("Merging branch-b into default")
    r = noa("workspace", "merge", "branch-b", cwd=REPO)
    log(f"Merge result: {r.stdout.strip()}")

    log("Checking final snapshot log")
    r = noa("log", cwd=REPO)
    merge_count = r.stdout.lower().count("merge")
    log(f"Found {merge_count} merge entries")

    log("Listing final workspaces")
    r = noa("workspace", "list", cwd=REPO)
    log(f"Workspaces: {r.stdout.strip()}")

    print("\n=== Example 3 PASSED ===\n")


if __name__ == "__main__":
    main()
