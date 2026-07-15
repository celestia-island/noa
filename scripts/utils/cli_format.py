"""Lightweight CLI formatting helpers for noa scripts."""

import os
import sys

_BRIGHT_GREEN = "\033[1;32m"
_BRIGHT_RED = "\033[1;31m"
_BRIGHT_YELLOW = "\033[1;33m"
_BRIGHT_CYAN = "\033[1;36m"
_DIM = "\033[2m"
_BOLD = "\033[1m"
_NC = "\033[0m"

_on: bool | None = None


def _color_on() -> bool:
    global _on
    if _on is None:
        _on = (
            hasattr(sys.stdout, "isatty")
            and sys.stdout.isatty()
            and not os.environ.get("NO_COLOR")
            and os.environ.get("TERM", "") != "dumb"
        )
    return _on


def _c(code: str, text: str) -> str:
    return f"{code}{text}{_NC}" if _color_on() else text


def ok(msg: str):
    print(f"{_c(_BRIGHT_GREEN, '[  OK  ]')} {msg}")


def info(msg: str):
    print(f"{_c(_BRIGHT_CYAN, '[ INFO ]')} {msg}")


def warn(msg: str):
    print(f"{_c(_BRIGHT_YELLOW, '[ WARN ]')} {msg}")


def fail(msg: str):
    print(f"{_c(_BRIGHT_RED, '[ FAIL ]')} {msg}", file=sys.stderr)


def bold(msg: str):
    print(_c(_BOLD, msg))


def blank():
    print()


def separator():
    pass


def header(title: str):
    blank()
    bold(title)


def step(title: str):
    bold(f"==> {title}")


err = fail
