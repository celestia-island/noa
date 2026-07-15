#!/usr/bin/env python3
"""Example 4: Remote sync — remote add, list, remove workflow."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent / "scripts" / "tests"))
from utils import noa, init_repo, log

REPO = Path("/tmp/noa-example-remote")


def main() -> None:
    print("=== Example 4: Remote Sync ===")

    log("Initializing repo")
    init_repo(REPO)

    log("Adding multiple remotes")
    remotes = [
        ("origin", "https://github.com/example/project.git"),
        ("upstream", "https://github.com/upstream/project.git"),
        ("mirror", "https://gitlab.com/example/project.git"),
    ]
    for name, url in remotes:
        noa("remote", "add", name, url, cwd=REPO)
        log(f"  Added {name} -> {url}")

    r = noa("remote", "list", cwd=REPO)
    for name, _ in remotes:
        assert name in r.stdout, f"remote {name} not listed"
    log(f"Remotes: {r.stdout.strip()}")

    log("Removing 'mirror' remote")
    noa("remote", "remove", "mirror", cwd=REPO)

    r = noa("remote", "list", cwd=REPO)
    assert "mirror" not in r.stdout, "mirror should be removed"
    assert "origin" in r.stdout, "origin should remain"
    log(f"After removal: {r.stdout.strip()}")

    log("Verifying config persistence")
<<<<<<< HEAD
    import configparser
=======
>>>>>>> origin/dev
    config_path = REPO / ".noa" / "config"
    content = config_path.read_text()
    assert "origin" in content, "origin not persisted in config"
    assert "upstream" in content, "upstream not persisted in config"
    log("Config file verified")

    print("\n=== Example 4 PASSED ===\n")


if __name__ == "__main__":
    main()
