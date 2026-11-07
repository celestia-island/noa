from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Optional

NOA_BIN = Path(os.environ.get("NOA_BIN", str(Path(__file__).resolve().parent.parent.parent / "target" / "debug" / "noa")))


def noa(*args: str, cwd: Optional[Path] = None, check: bool = True) -> subprocess.CompletedProcess[str]:
    cmd = [str(NOA_BIN)] + list(args)
    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=str(cwd) if cwd else None,
        check=False,
    )
    if check and result.returncode != 0:
        print(f"FAIL: noa {' '.join(args)}", file=sys.stderr)
        print(f"  stdout: {result.stdout.strip()}", file=sys.stderr)
        print(f"  stderr: {result.stderr.strip()}", file=sys.stderr)
        raise RuntimeError(f"noa command failed with exit code {result.returncode}")
    return result


def init_repo(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)
    noa("init", str(path), cwd=path)


def write_file(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def read_file(path: Path) -> str:
    return path.read_text()


def log(msg: str) -> None:
    print(f"  {msg}")
