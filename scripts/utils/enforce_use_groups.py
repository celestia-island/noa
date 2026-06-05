#!/usr/bin/env python3
"""Enforce workspace-specific `use` statement layout rules for noa.

The policy is described in the repo conventions and can be summarized as:

1. Group imports into three sections: shared utility crates (`std`, `anyhow`,
   `serde`, ...), domain-specific external crates (e.g. `axum`, `gix`, `aws_*`),
   and workspace/internal crates (`crate::`, `super::`).

2. Place a single blank line between groups and after the final group before code.
3. In `mod.rs`, `lib.rs`, and `main.rs`, emit all `mod` declarations before the
   first `use` block.
4. Merge consecutive simple paths that share a prefix.

This script rewrites files in-place and prints a summary of modified files.
"""

from __future__ import annotations

import os
import re
import sys
from collections import OrderedDict
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, Sequence, Tuple
import shutil
import subprocess
import cli_format as cf

try:  # Python 3.11+
    import tomllib  # type: ignore
except ModuleNotFoundError:  # pragma: no cover
    import tomli as tomllib  # type: ignore

GROUP1_CRATES = {
    "std",
    "core",
    "alloc",
    "anyhow",
    "thiserror",
    "serde",
    "serde_json",
    "rmp_serde",
    "toml",
    "clap",
    "tokio",
    "async_trait",
    "chrono",
    "uuid",
    "tracing",
    "tracing_subscriber",
    "base64",
    "hex",
    "bytes",
    "regex",
    "lazy_static",
    "once_cell",
    "url",
    "parking_lot",
    "rayon",
    "log",
    "rand",
}

USE_RE = re.compile(r"^\s*(pub\s+)?use\b")
MOD_RE = re.compile(r"^\s*(pub\s+)?mod\b")
ATTR_RE = re.compile(r"^\s*#\[")
COMMENT_RE = re.compile(r"^\s*//")
BLOCK_COMMENT_START_RE = re.compile(r"^\s*/\*")

WORKSPACE_CRATES: set[str] = set()


@dataclass
class UseStatement:
    lines: List[str]
    path: Optional[str]
    is_pub: bool
    group: int
    simple_prefix: Optional[str]
    simple_leaf: Optional[str]
    has_attrs: bool
    merge_base: Optional[str]
    merge_remainder: Optional[str]

    def text(self) -> str:
        return "".join(self.lines)


@dataclass
class Statement:
    kind: str
    lines: List[str]
    use_stmt: Optional[UseStatement] = None


def load_workspace_crates(root: Path) -> set[str]:
    crates: set[str] = set()
    for dirpath, dirnames, filenames in os.walk(root):
        if "target" in dirnames:
            dirnames.remove("target")
        if "Cargo.toml" not in filenames:
            continue
        path = Path(dirpath) / "Cargo.toml"
        try:
            with path.open("rb") as fh:
                data = tomllib.load(fh)
        except Exception:
            continue
        package = data.get("package")
        if package and "name" in package:
            crates.add(package["name"])
    return crates


def strip_line_comments(line: str) -> str:
    idx = line.find("//")
    if idx == -1:
        return line

    before_comment = line[:idx]
    after_comment = line[idx:]

    suffix = ""

    open_braces = before_comment.count("{")
    close_braces = before_comment.count("}")

    if "}" in after_comment and open_braces > close_braces:
        needed_closes = open_braces - close_braces
        for _ in range(needed_closes):
            suffix += "}"

    if ";" in after_comment:
        suffix += ";"

    result = before_comment.rstrip()
    if suffix:
        result += suffix

    return result


def extract_use_path(lines: Sequence[str]) -> Optional[str]:
    cleaned_lines = []
    for line in lines:
        if ATTR_RE.match(line):
            continue
        cleaned = strip_line_comments(line).strip()
        if cleaned:
            cleaned_lines.append(cleaned)
    joined = " ".join(cleaned_lines)
    match = re.search(r"\buse\s+([^;]+);", joined)
    return match.group(1).strip() if match else None


def extract_base_crate(path: str) -> str:
    token = path.strip()
    if token.startswith("pub "):
        token = token[4:].strip()
    if token.startswith("use "):
        token = token[4:].strip()
    token = token.lstrip(":").strip()
    match = re.match(r'^([a-zA-Z_][a-zA-Z0-9_]*)', token)
    if match:
        return match.group(1)
    return ""


def classify_use(path: Optional[str], workspace_crates: set[str]) -> int:
    if not path:
        return 2
    base = extract_base_crate(path)
    if not base:
        return 2
    if base in ("crate", "self", "super"):
        return 3
    if base == "libnoa":
        return 3
    if base.startswith("_"):
        return 3
    if base in workspace_crates:
        return 3
    if base in GROUP1_CRATES:
        return 1
    return 2


def compute_simple_components(path: Optional[str], has_attrs: bool) -> Tuple[Optional[str], Optional[str]]:
    if has_attrs or not path:
        return None, None
    token = path.strip()
    if any(ch in token for ch in "{}*"):
        return None, None
    if " as " in token:
        return None, None
    if token.endswith(":"):
        return None, None
    parts = token.split("::")
    if len(parts) < 2:
        return None, None
    prefix = "::".join(parts[:-1])
    leaf = parts[-1].strip()
    if not prefix or not leaf:
        return None, None
    return prefix, leaf


def compute_merge_components(path: Optional[str], has_attrs: bool) -> Tuple[Optional[str], Optional[str]]:
    if has_attrs or not path:
        return None, None
    base = extract_base_crate(path)
    if not base:
        return None, None
    token = path.strip()
    if token.startswith("pub "):
        token = token[4:].strip()
    if token.startswith("use "):
        token = token[4:].strip()
    token = token.lstrip(":").strip()
    if token.startswith(base):
        if len(token) > len(base) and token[len(base):len(base)+2] == "::":
            remainder = token[len(base)+2:]
            return base, remainder
        elif len(token) == len(base):
            return base, ""
    return None, None


def append_blank_line(buf: List[str]) -> None:
    if not buf:
        return
    if buf[-1].strip():
        buf.append("\n")


def collect_statement(lines: List[str], idx: int) -> Tuple[Optional[Statement], int]:
    attrs: List[str] = []
    cur = idx
    while cur < len(lines) and ATTR_RE.match(lines[cur]):
        attrs.append(lines[cur])
        cur += 1
    if cur >= len(lines):
        return None, idx
    line = lines[cur]
    if USE_RE.match(line):
        stmt_lines = attrs + [line]
        cur += 1
        brace_balance = line.count("{") - line.count("}")
        semicolon_found = ";" in line and brace_balance <= 0
        while not semicolon_found and cur < len(lines):
            stmt_lines.append(lines[cur])
            brace_balance += lines[cur].count("{") - lines[cur].count("}")
            if ";" in lines[cur] and brace_balance <= 0:
                semicolon_found = True
            cur += 1
        use_stmt = build_use_statement(stmt_lines)
        return Statement("use", stmt_lines, use_stmt), cur
    if MOD_RE.match(line):
        if "{" in line and "}" not in line:
            return None, idx
        stmt_lines = attrs + [line]
        cur += 1
        return Statement("mod", stmt_lines), cur
    return None, idx


def build_use_statement(lines: List[str]) -> UseStatement:
    code_lines = [line for line in lines if not ATTR_RE.match(line)]
    path = extract_use_path(lines)
    is_pub = bool(code_lines and code_lines[0].lstrip().startswith("pub "))
    has_attrs = any(ATTR_RE.match(line) for line in lines)
    prefix, leaf = compute_simple_components(path, has_attrs)
    merge_base, merge_remainder = compute_merge_components(path, has_attrs)
    group = classify_use(path, WORKSPACE_CRATES)
    return UseStatement(lines, path, is_pub, group, prefix, leaf, has_attrs, merge_base, merge_remainder)


def normalize_remainder_items(remainder: str) -> List[str]:
    remainder = remainder.strip()
    if remainder.startswith("{") and remainder.endswith("}"):
        inner = remainder[1:-1]
        parts: List[str] = []
        cur: List[str] = []
        depth = 0
        for ch in inner:
            if ch == '{':
                depth += 1
                cur.append(ch)
            elif ch == '}':
                depth -= 1
                cur.append(ch)
            elif ch == ',' and depth == 0:
                part = ''.join(cur).strip()
                if part:
                    parts.append(part)
                cur = []
            else:
                cur.append(ch)
        last = ''.join(cur).strip()
        if last:
            parts.append(last)
        return parts
    if remainder == "":
        return []
    return [remainder]


def find_matching_brace(s: str, start: int) -> int:
    if start >= len(s) or s[start] != '{':
        return -1
    depth = 0
    for i in range(start, len(s)):
        if s[i] == '{':
            depth += 1
        elif s[i] == '}':
            depth -= 1
            if depth == 0:
                return i
    return -1


def expand_braced_item(item: str) -> List[str]:
    if item == "":
        return [""]

    brace_start = item.find('{')
    if brace_start == -1:
        return [item]

    if brace_start < 2 or item[brace_start-2:brace_start] != "::":
        return [item]

    brace_end = find_matching_brace(item, brace_start)
    if brace_end == -1:
        return [item]

    prefix = item[:brace_start-2]
    braced_content = item[brace_start:brace_end+1]
    suffix = item[brace_end+1:]

    inner_items = normalize_remainder_items(braced_content)

    expanded: List[str] = []
    for inner in inner_items:
        if inner == "":
            new_item = prefix + suffix
        else:
            new_item = f"{prefix}::{inner}{suffix}"
        expanded.extend(expand_braced_item(new_item))

    return expanded


def expand_braced_items(items: List[str]) -> List[str]:
    expanded: List[str] = []
    for item in items:
        expanded.extend(expand_braced_item(item))
    return expanded


def build_import_tree(items: List[str]) -> dict:
    expanded_items = expand_braced_items(items)
    tree: dict = {}
    for item in expanded_items:
        if item == "":
            tree[""] = {}
            continue
        parts = item.split("::")
        current = tree
        for part in parts:
            if part not in current:
                current[part] = {}
            current = current[part]
    for item in expanded_items:
        if item == "":
            continue
        parts = item.split("::")
        current = tree
        for part in parts:
            current = current[part]
        if current:
            current[""] = {}
    return tree


def format_import_tree(tree: dict, indent: int = 0) -> str:
    if not tree:
        return ""
    has_self = "" in tree
    keys = sorted([k for k in tree.keys() if k != ""])
    if not keys:
        return ""
    if len(keys) == 1 and not tree[keys[0]] and not has_self:
        return keys[0]
    parts: List[str] = []
    if has_self:
        parts.append("self")
    for key in keys:
        subtree = tree[key]
        if not subtree:
            parts.append(key)
        else:
            child_str = format_import_tree(subtree)
            if child_str:
                parts.append(f"{key}::{child_str}")
            else:
                parts.append(key)
    if len(parts) == 1 and not has_self:
        return parts[0]
    return "{" + ", ".join(parts) + "}"


def find_common_prefix(paths: List[str]) -> str:
    if not paths:
        return ""
    if len(paths) == 1:
        if "::" in paths[0]:
            return "::".join(paths[0].split("::")[:-1])
        return ""
    all_parts = [p.split("::") if p else [] for p in paths]
    if not all_parts or not all(parts for parts in all_parts):
        return ""
    prefix_parts: List[str] = []
    for i in range(min(len(parts) for parts in all_parts)):
        part = all_parts[0][i]
        if all(parts[i] == part for parts in all_parts):
            prefix_parts.append(part)
        else:
            break
    return "::".join(prefix_parts)


def render_group(statements: List[UseStatement]) -> List[str]:
    if not statements:
        return []
    output: List[str] = []
    pending: OrderedDict[Tuple[bool, str], List[str]] = OrderedDict()
    for stmt in statements:
        if stmt.merge_base and stmt.merge_remainder is not None and not stmt.has_attrs:
            key = (stmt.is_pub, stmt.merge_base)
            if key not in pending:
                pending[key] = []
            items = normalize_remainder_items(stmt.merge_remainder)
            if items:
                pending[key].extend(items)
            else:
                pending[key].append("")
            continue
        flush_merged_groups(pending, output)
        output.extend(stmt.lines)
    flush_merged_groups(pending, output)
    return output


def flush_merged_groups(pending: OrderedDict[Tuple[bool, str], List[str]], output: List[str]) -> None:
    for (is_pub, base), items in pending.items():
        if not items:
            continue
        common_prefix = find_common_prefix(items)
        if common_prefix:
            new_base = f"{base}::{common_prefix}"
            new_items: List[str] = []
            for item in items:
                if item == "":
                    new_items.append("")
                elif item == common_prefix:
                    new_items.append("")
                elif item.startswith(common_prefix + "::"):
                    new_items.append(item[len(common_prefix) + 2:])
                else:
                    new_items.append(item)
            tree = build_import_tree(new_items)
            formatted = format_import_tree(tree)
            if formatted:
                line = f"{'pub ' if is_pub else ''}use {new_base}::{formatted};\n"
            else:
                line = f"{'pub ' if is_pub else ''}use {new_base};\n"
        else:
            tree = build_import_tree(items)
            formatted = format_import_tree(tree)
            if formatted:
                line = f"{'pub ' if is_pub else ''}use {base}::{formatted};\n"
            else:
                line = f"{'pub ' if is_pub else ''}use {base};\n"
        output.append(line)
    pending.clear()


def render_use_section(use_statements: List[UseStatement]) -> List[str]:
    grouped: dict[int, List[UseStatement]] = {1: [], 2: [], 3: []}
    for stmt in use_statements:
        grouped[stmt.group].append(stmt)
    rendered: List[str] = []
    for group in (1, 2, 3):
        block = render_group(grouped[group])
        if not block:
            continue
        if rendered and rendered[-1].strip():
            rendered.append("\n")
        rendered.extend(block)
    if rendered and rendered[-1].strip():
        rendered.append("\n")
    return rendered


def process_file(path: Path) -> Optional[str]:
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
    idx = 0
    prefix: List[str] = []
    while idx < len(lines):
        stripped = lines[idx].strip()
        if stripped == "" or COMMENT_RE.match(lines[idx]) or BLOCK_COMMENT_START_RE.match(lines[idx]):
            prefix.append(lines[idx])
            idx += 1
            continue
        if lines[idx].startswith("#!"):
            prefix.append(lines[idx])
            idx += 1
            continue
        if ATTR_RE.match(lines[idx]):
            break
        if USE_RE.match(lines[idx]) or MOD_RE.match(lines[idx]):
            break
        return None
    statements: List[Statement] = []
    cur = idx
    while cur < len(lines):
        stmt, next_idx = collect_statement(lines, cur)
        if stmt is None:
            break
        statements.append(stmt)
        cur = next_idx
        while cur < len(lines) and lines[cur].strip() == "":
            cur += 1
    suffix = lines[cur:]
    if not statements:
        return None
    use_statements = [
        stmt.use_stmt for stmt in statements if stmt.kind == "use" and stmt.use_stmt]
    if not use_statements:
        return None
    use_section = render_use_section(use_statements)
    if not use_section:
        return None
    require_mod_first = path.name in {"mod.rs", "lib.rs", "main.rs"}
    new_lines: List[str] = []
    new_lines.extend(prefix)
    if require_mod_first:
        mods = [stmt.lines for stmt in statements if stmt.kind == "mod"]
        for mod_lines in mods:
            new_lines.extend(mod_lines)
        if mods and use_section:
            append_blank_line(new_lines)
        new_lines.extend(use_section)
    else:
        others_written = False
        for stmt in statements:
            if stmt.kind == "use":
                continue
            new_lines.extend(stmt.lines)
            others_written = True
        if others_written and use_section:
            append_blank_line(new_lines)
        new_lines.extend(use_section)
    new_lines.extend(suffix)
    if new_lines and not new_lines[-1].endswith("\n"):
        new_lines[-1] += "\n"
    new_text = "".join(new_lines)
    original_text = "".join(lines)
    return new_text if new_text != original_text else None


def main() -> int:
    cf.header("ENFORCE USE STATEMENT GROUPS")
    print(f"Working directory: {Path.cwd()}")
    print(f"Python version: {sys.version}")
    changed: List[str] = []
    total_files = 0
    for path in Path.cwd().rglob("*.rs"):
        if "target" in path.parts:
            continue
        total_files += 1
        try:
            new_text = process_file(path)
            if new_text is not None:
                path.write_text(new_text, encoding="utf-8")
                changed.append(str(path))
        except Exception as e:
            print(f"Error processing {path}: {e}")
            import traceback
            traceback.print_exc()
            return 1
    cf.ok(f"Processed {total_files} Rust files")
    cf.ok(f"Updated {len(changed)} files")
    for item in changed:
        print(item)
    try:
        if shutil.which("cargo"):
            cf.step("Running cargo fmt")
            result = subprocess.run(
                ["cargo", "fmt"],
                check=False,
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
            )
            if result.returncode != 0:
                cf.fail(f"cargo fmt exit code: {result.returncode}")
                if result.stderr:
                    print(f"cargo fmt stderr: {result.stderr}")
        else:
            cf.warn("cargo not found in PATH; skipping cargo fmt")
    except Exception as e:
        cf.fail(f"Failed to run cargo fmt: {e}")
    cf.ok("Use statement enforcement completed successfully")
    return 0


def test_classification() -> bool:
    test_cases = [
        ("std::collections::HashMap", 1, "std library"),
        ("anyhow::Result", 1, "anyhow utility"),
        ("tokio::spawn", 1, "tokio runtime"),
        ("serde_json::Value", 1, "serde_json utility"),
        ("clap::Parser", 1, "clap utility"),
        ("tracing::info", 1, "tracing utility"),
        ("chrono::Utc", 1, "chrono utility"),
        ("uuid::Uuid", 1, "uuid utility"),
        ("gix::Repository", 2, "gix external"),
        ("axum::Router", 2, "axum external"),
        ("aws_sdk_s3::Client", 2, "aws-sdk-s3 external"),
        ("redb::Database", 2, "redb external"),
        ("sha2::Sha256", 2, "sha2 external"),
        ("ignore::WalkBuilder", 2, "ignore external"),
        ("tower::Service", 2, "tower external"),
        ("crate::config::Config", 3, "crate prefix"),
        ("self::item", 3, "self prefix"),
        ("super::parent", 3, "super prefix"),
        ("libnoa::something", 3, "libnoa workspace crate"),
    ]
    workspace_crates: set[str] = set()
    all_passed = True
    cf.header("IMPORT GROUP CLASSIFICATION TEST")
    print(f"\n{'Test Case':<50} | {'Expected':<10} | {'Actual':<10} | Status")
    cf.separator()
    group_names = {1: "std/util", 2: "external", 3: "internal"}
    for test_input, expected_group, description in test_cases:
        actual_group = classify_use(test_input, workspace_crates)
        passed = actual_group == expected_group
        status = "PASS" if passed else "FAIL"
        expected_name = group_names[expected_group]
        actual_name = group_names[actual_group]
        print(f"{description:<50} | {expected_name:<10} | {actual_name:<10} | {status}")
        if not passed:
            all_passed = False
            base = extract_base_crate(test_input)
            print(f"  -> Input: '{test_input}'")
            print(f"  -> Base crate: '{base}'")
    cf.separator()
    if all_passed:
        print("\nAll classification tests PASSED!")
    else:
        print("\nSome tests FAILED!")
    cf.separator()
    print()
    return all_passed


if __name__ == "__main__":
    try:
        if "--test" in sys.argv:
            print("Running in test mode...")
            WORKSPACE_CRATES = load_workspace_crates(Path.cwd())
            passed = test_classification()
            sys.exit(0 if passed else 1)
        print("Loading workspace crates...")
        WORKSPACE_CRATES = load_workspace_crates(Path.cwd())
        print(f"Found {len(WORKSPACE_CRATES)} workspace crates")
        exit_code = main()
        print(f"Exiting with code: {exit_code}")
        sys.exit(exit_code)
    except Exception as e:
        print(f"FATAL ERROR: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)
