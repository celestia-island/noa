from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

sys.path.insert(0, str(ROOT / "scripts" / "utils"))
try:
    import logger as _log
    _log = _log.Logger(source="noa", module="just_tasks")
except ImportError:
    _log = None


def run_command(
    cmd: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    capture_output: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd or ROOT,
        env=env,
        capture_output=capture_output,
        text=True,
        check=check,
    )


def configure_stdout() -> None:
    try:
        sys.stdout.reconfigure(line_buffering=True)
    except AttributeError:
        pass


def log_ok(msg: str) -> None:
    if _log:
        _log.ok(msg)
    else:
        print(f"\033[32m{msg}\033[0m", flush=True)


def log_warn(msg: str) -> None:
    if _log:
        _log.warn(msg)
    else:
        print(f"\033[33m{msg}\033[0m", flush=True)


def log_info(msg: str) -> None:
    if _log:
        _log.info(msg)
    else:
        print(f"\033[36m{msg}\033[0m", flush=True)


def log_blank() -> None:
    if _log:
        _log.blank()
    else:
        print(flush=True)


def main() -> None:
    configure_stdout()
    if len(sys.argv) < 2:
        print("Usage: just_tasks.py <command> [args...]")
        sys.exit(1)

    command = sys.argv[1]
    log_info(f"Running: {command}")
    print(f"Unknown command: {command}", file=sys.stderr)
    sys.exit(1)


if __name__ == "__main__":
    main()
