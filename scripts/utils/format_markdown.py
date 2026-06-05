#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


def format_markdown_file(path: Path, check: bool = False) -> bool:
    content = path.read_text(encoding="utf-8")
    original = content

    lines = content.split("\n")
    formatted_lines: list[str] = []
    for line in lines:
        formatted_lines.append(line.rstrip())

    content = "\n".join(formatted_lines)

    if content and not content.endswith("\n"):
        content += "\n"

    if content != original:
        if check:
            return False
        path.write_text(content, encoding="utf-8")
    return True


def main() -> None:
    parser = argparse.ArgumentParser(description="Format markdown files")
    parser.add_argument("path", type=Path, help="Directory to format")
    parser.add_argument("--check", action="store_true", help="Check only")
    args = parser.parse_args()

    root = args.path
    if not root.is_dir():
        print(f"Error: {root} is not a directory", file=sys.stderr)
        sys.exit(1)

    all_ok = True
    for md_file in sorted(root.rglob("*.md")):
        if ".noa" in md_file.parts or "target" in md_file.parts:
            continue
        ok = format_markdown_file(md_file, check=args.check)
        if not ok:
            print(f"would reformat: {md_file}")
            all_ok = False

    sys.exit(0 if all_ok else 1)


if __name__ == "__main__":
    main()
