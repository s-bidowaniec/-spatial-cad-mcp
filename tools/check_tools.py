"""Lint MCP tool definitions for AI-agent consumers.

Scans packages/*/src/server.rs (Rust, rmcp) and packages/mcp-ifc (Python,
FastMCP) and checks, per tool:

- name: snake_case, 3-64 chars, unique per server, prefixed with its group
  (ifc_ / dxf_ / gis_ / hdm_ / mesh_);
- description: present, 80-2048 chars, states the kind ("Read-only ..." or
  "Mutating tool: ...");
- annotations: all four hints explicitly set (read_only_hint / readOnlyHint,
  destructive_hint / destructiveHint, idempotent_hint / idempotentHint,
  open_world_hint / openWorldHint); read-only must match the stated kind;
  destructive and open-world must be false (no destructive writes, no network);
- server instructions: Rust servers set #[tool_handler(instructions = ...)],
  the Python server sets FastMCP(instructions=...).

Print problems to stdout and return 1 on failure, or 0 when clean.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

GROUP_PREFIXES = {
    "mcp-ifc": "ifc_",
    "mcp-dxf": "dxf_",
    "mcp-gis": "gis_",
    "mcp-hdm": "hdm_",
    "mcp-mesh": "mesh_",
}

NAME_RE = re.compile(r"^[a-z][a-z0-9_]{2,63}$")
DESC_RE = re.compile(r'description = "(.*)"\s*,?\s*$')
FLAG_RE = re.compile(
    r"(read_only_hint|destructive_hint|idempotent_hint|open_world_hint)\s*=\s*(true|false)"
)
PY_DECORATOR_RE = re.compile(r"@mcp\.tool\((.*?)\)\s*\n\s*def (\w+)\(", re.S)
PY_DOC_RE = re.compile(r'def \w+\([^)]*\)[^:]*:\s*"""(.*?)"""', re.S)
PY_FLAG_RE = re.compile(
    r"(readOnlyHint|destructiveHint|idempotentHint|openWorldHint)\s*=\s*(True|False)"
)

MIN_DESC, MAX_DESC = 80, 2048


def _kind_of(description: str) -> str | None:
    low = description.lower()
    if low.startswith("read-only"):
        return "read-only"
    if low.startswith("mutating"):
        return "mutating"
    return None


def check_rust_file(path: Path, group: str) -> list[str]:
    prefix = GROUP_PREFIXES[group]
    rel = path.relative_to(path.parents[3]).as_posix()
    text = path.read_text(encoding="utf-8")
    problems: list[str] = []
    if "#[rmcp::tool_handler(instructions = " not in text and "instructions = " not in text:
        problems.append(f"{rel}: server is missing #[tool_handler(instructions = ...)]")
    lines = text.splitlines()
    names: list[str] = []
    i = 0
    while i < len(lines):
        if lines[i].strip() == "#[tool(":
            block: list[str] = []
            i += 1
            while i < len(lines) and lines[i].strip() != ")]":
                block.append(lines[i])
                i += 1
            i += 1
            j = i
            while j < len(lines) and not re.match(r"\s*pub fn (\w+)", lines[j]):
                j += 1
            if j >= len(lines):
                problems.append(f"{rel}: #[tool] block without a following pub fn")
                continue
            name = re.match(r"\s*pub fn (\w+)", lines[j]).group(1)
            names.append(name)
            desc = next((m.group(1) for b in block if (m := DESC_RE.search(b))), None)
            ann_text = "\n".join(block)
            problems.extend(_check_tool(rel, name, desc, ann_text, prefix, rust=True))
            i = j
        else:
            i += 1
    for name in names:
        if names.count(name) > 1:
            problems.append(f"{rel}: duplicate tool name `{name}`")
    return problems


def check_python_file(path: Path) -> list[str]:
    rel = path.relative_to(path.parents[3]).as_posix()
    text = path.read_text(encoding="utf-8")
    problems: list[str] = []
    if "instructions=" not in text:
        problems.append(f"{rel}: FastMCP server is missing instructions=")
    doc_re = re.compile(r'\)\s*->[^:]*:\s*"""(.*?)"""', re.S)
    names: list[str] = []
    for match in PY_DECORATOR_RE.finditer(text):
        args, name = match.group(1), match.group(2)
        names.append(name)
        doc = doc_re.search(text, match.end())
        desc = doc.group(1).strip() if doc else None
        problems.extend(_check_tool(rel, name, desc, args, "ifc_", rust=False))
    for name in names:
        if names.count(name) > 1:
            problems.append(f"{rel}: duplicate tool name `{name}`")
    return problems


def _check_tool(
    rel: str, name: str, desc: str | None, ann_text: str, prefix: str, *, rust: bool
) -> list[str]:
    problems: list[str] = []
    label = f"{rel}: tool `{name}`"
    if not NAME_RE.match(name):
        problems.append(f"{label}: name is not snake_case (3-64 chars)")
    if not name.startswith(prefix):
        problems.append(f"{label}: name must start with `{prefix}`")
    if desc is None:
        problems.append(f"{label}: missing description")
    else:
        if not (MIN_DESC <= len(desc) <= MAX_DESC):
            problems.append(
                f"{label}: description length {len(desc)} outside {MIN_DESC}-{MAX_DESC}"
            )
        kind = _kind_of(desc) if rust else None
        if rust and kind is None:
            problems.append(
                f"{label}: description must state the kind (Read-only ... / Mutating tool: ...)"
            )
    flags = dict(FLAG_RE.findall(ann_text)) if rust else dict(PY_FLAG_RE.findall(ann_text))
    expected = (
        {"read_only_hint", "destructive_hint", "idempotent_hint", "open_world_hint"}
        if rust
        else {"readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint"}
    )
    missing = expected - set(flags)
    if missing:
        problems.append(f"{label}: annotations missing {sorted(missing)}")
        return problems
    ro_key = "read_only_hint" if rust else "readOnlyHint"
    de_key = "destructive_hint" if rust else "destructiveHint"
    id_key = "idempotent_hint" if rust else "idempotentHint"
    ow_key = "open_world_hint" if rust else "openWorldHint"
    ro = flags[ro_key] == ("true" if rust else "True")
    if rust and desc is not None:
        kind = _kind_of(desc)
        if kind == "read-only" and not ro:
            problems.append(f"{label}: read-only description but read_only_hint is false")
        if kind == "mutating" and ro:
            problems.append(f"{label}: mutating description but read_only_hint is true")
    if flags[de_key] != ("false" if rust else "False"):
        problems.append(
            f"{label}: destructive_hint must be false (mutators are additive, outputs/ only)"
        )
    if flags[ow_key] != ("false" if rust else "False"):
        problems.append(f"{label}: open_world_hint must be false (no network egress)")
    idem = flags[id_key] == ("true" if rust else "True")
    if ro != idem:
        problems.append(f"{label}: idempotent_hint should match read_only_hint")
    return problems


def check_tree(root: Path) -> list[str]:
    root = root.resolve()
    problems: list[str] = []
    for server_dir in sorted((root / "packages").iterdir()):
        if not server_dir.is_dir() or server_dir.name not in GROUP_PREFIXES:
            continue
        server_rs = server_dir / "src" / "server.rs"
        if server_rs.is_file():
            problems.extend(check_rust_file(server_rs, server_dir.name))
    ifc_server = root / "packages" / "mcp-ifc" / "src" / "mcp_ifc" / "server.py"
    if ifc_server.is_file():
        problems.extend(check_python_file(ifc_server))
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    problems = check_tree(args.root)
    for problem in problems:
        print(problem)
    if problems:
        print(f"FAILED: {len(problems)} tool-definition problem(s) under {args.root}")
        return 1
    print(
        f"OK: tool definitions (names, descriptions, annotations, instructions)"
        f" hold under {args.root}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
