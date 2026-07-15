#!/usr/bin/env python3
"""Run all noa example scenarios."""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

EXAMPLES = [
    ("basic-workflow", "Basic init/workspace/snapshot/log workflow"),
    ("multi-agent", "Multi-agent concurrent workspace + snapshot"),
    ("merge-scenarios", "Workspace creation, merging, conflict detection"),
    ("remote-sync", "Remote add/list/remove + config persistence"),
]

EXAMPLES_DIR = Path(__file__).resolve().parent


def main() -> None:
    print("=" * 60)
    print("noa — Running all example scenarios")
    print("=" * 60)

    results: list[tuple[str, bool, str]] = []

    for name, desc in EXAMPLES:
        script = EXAMPLES_DIR / name / "run.py"
        if not script.exists():
            print(f"\n  SKIP {name}: script not found at {script}")
            continue

        print(f"\n--- {name}: {desc} ---")
        result = subprocess.run(
            [sys.executable, str(script)],
            capture_output=True,
            text=True,
        )
        passed = result.returncode == 0
        results.append((name, passed, result.stdout + result.stderr))

        if passed:
<<<<<<< HEAD
            print(f"  PASS")
        else:
            print(f"  FAIL")
=======
            print("  PASS")
        else:
            print("  FAIL")
>>>>>>> origin/dev
            print(result.stdout)
            print(result.stderr)

    print("\n" + "=" * 60)
    print("Summary")
    print("=" * 60)
    for name, passed, _ in results:
        status = "PASS" if passed else "FAIL"
        print(f"  [{status}] {name}")

    all_passed = all(p for _, p, _ in results)
    print()
    if all_passed:
        print("All examples passed!")
    else:
        print("Some examples failed.")
        sys.exit(1)


if __name__ == "__main__":
    main()
