"""Tests for tools/check_tools.py on synthetic tool definitions."""

import importlib.util
import re
from pathlib import Path
from types import ModuleType

import pytest

SCRIPT = Path(__file__).resolve().parents[1] / "tools" / "check_tools.py"

_spec = importlib.util.spec_from_file_location("check_tools", SCRIPT)
assert _spec is not None and _spec.loader is not None
check_tools: ModuleType = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(check_tools)

DESC = "Read-only orientation summary of a DXF drawing with counts and extents. Call first."
MUT_DESC = "Mutating tool: writes a new DXF under outputs. Never modifies the input file."
ANN = """annotations(
            title = "DXF summary",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )"""

GOOD_RUST = f'''#[tool_router]
impl DxfServer {{
    #[tool(
        description = "{DESC}",
        {ANN}
    )]
    pub fn dxf_summary(&self) -> String {{
        String::new()
    }}
}}

#[rmcp::tool_handler(instructions = "Start with dxf_summary.")]
impl rmcp::handler::server::ServerHandler for DxfServer {{}}
'''

PY_ANN = """annotations=ToolAnnotations(
        readOnlyHint=True, destructiveHint=False, idempotentHint=True, openWorldHint=False
    )"""

GOOD_PY = f'''from mcp.server.fastmcp import FastMCP

mcp = FastMCP(name="mcp-ifc", instructions="Start with ifc_summary.")


@mcp.tool(
    {PY_ANN}
)
def ifc_summary(ifc_path: str) -> dict:
    """{DESC}

    Args:
        ifc_path: Path to the .ifc file relative to the workspace root.
    """
    return {{}}
'''


def write(tmp_path: Path, rel: str, content: str) -> Path:
    path = tmp_path / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    return path


def test_good_rust_tool_passes(tmp_path: Path) -> None:
    path = write(tmp_path, "packages/mcp-dxf/src/server.rs", GOOD_RUST)
    assert check_tools.check_rust_file(path, "mcp-dxf") == []


def test_good_python_tool_passes(tmp_path: Path) -> None:
    path = write(tmp_path, "packages/mcp-ifc/src/mcp_ifc/server.py", GOOD_PY)
    assert check_tools.check_python_file(path) == []


def test_missing_annotations_is_reported(tmp_path: Path) -> None:
    bad = re.sub(r"        annotations\(.*?\n        \)\n?", "", GOOD_RUST, flags=re.S)
    assert "annotations(" not in bad
    path = write(tmp_path, "packages/mcp-dxf/src/server.rs", bad)
    found = check_tools.check_rust_file(path, "mcp-dxf")
    assert any("annotations missing" in p for p in found)


def test_short_description_is_reported(tmp_path: Path) -> None:
    bad = GOOD_RUST.replace(DESC, "Read-only summary.")
    path = write(tmp_path, "packages/mcp-dxf/src/server.rs", bad)
    found = check_tools.check_rust_file(path, "mcp-dxf")
    assert any("description length" in p for p in found)


def test_wrong_prefix_and_bad_name_are_reported(tmp_path: Path) -> None:
    bad = GOOD_RUST.replace("pub fn dxf_summary", "pub fn GetData")
    path = write(tmp_path, "packages/mcp-dxf/src/server.rs", bad)
    found = check_tools.check_rust_file(path, "mcp-dxf")
    assert any("snake_case" in p for p in found)
    assert any("dxf_" in p for p in found)


def test_mutating_description_requires_read_only_false(tmp_path: Path) -> None:
    bad = GOOD_RUST.replace(DESC, MUT_DESC)
    path = write(tmp_path, "packages/mcp-dxf/src/server.rs", bad)
    found = check_tools.check_rust_file(path, "mcp-dxf")
    assert any("mutating description but read_only_hint is true" in p for p in found)


def test_open_world_true_is_reported(tmp_path: Path) -> None:
    bad = GOOD_RUST.replace("open_world_hint = false", "open_world_hint = true")
    path = write(tmp_path, "packages/mcp-dxf/src/server.rs", bad)
    found = check_tools.check_rust_file(path, "mcp-dxf")
    assert any("open_world_hint must be false" in p for p in found)


def test_missing_server_instructions_are_reported(tmp_path: Path) -> None:
    bad = GOOD_RUST.replace('#[rmcp::tool_handler(instructions = "Start with dxf_summary.")]\n', "")
    path = write(tmp_path, "packages/mcp-dxf/src/server.rs", bad)
    found = check_tools.check_rust_file(path, "mcp-dxf")
    assert any("instructions" in p for p in found)


def test_duplicate_names_are_reported(tmp_path: Path) -> None:
    second = GOOD_RUST[GOOD_RUST.index("#[tool_router]") :]
    path = write(tmp_path, "packages/mcp-dxf/src/server.rs", GOOD_RUST + "\n" + second)
    found = check_tools.check_rust_file(path, "mcp-dxf")
    assert any("duplicate tool name" in p for p in found)


def test_python_missing_annotations_is_reported(tmp_path: Path) -> None:
    bad = GOOD_PY.replace(f"@mcp.tool(\n    {PY_ANN}\n)", "@mcp.tool()")
    assert "ToolAnnotations" not in bad
    path = write(tmp_path, "packages/mcp-ifc/src/mcp_ifc/server.py", bad)
    found = check_tools.check_python_file(path)
    assert any("annotations missing" in p for p in found)


def test_real_repo_tool_definitions_are_clean() -> None:
    root = Path(__file__).resolve().parents[1]
    assert check_tools.check_tree(root) == []


@pytest.mark.parametrize(
    ("description", "expected_kind"),
    [
        ("Read-only summary of things.", "read-only"),
        ("Mutating tool: writes a new file.", "mutating"),
        ("Does something useful.", None),
    ],
)
def test_kind_detection(description: str, expected_kind: str | None) -> None:
    assert check_tools._kind_of(description) == expected_kind
