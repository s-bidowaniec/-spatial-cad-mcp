"""FastMCP server entry point for mcp-ifc (stdio transport).

Starts an IFC inspection/query/validation server plus one mutating tool
(``ifc_export_submodel``). Paths given to tools are always resolved inside
``MCP_CAD_WORKSPACE_ROOT``.

Usage::

    MCP_CAD_WORKSPACE_ROOT=/abs/path uv run --project packages/mcp-ifc mcp-ifc

The MCP-exposed tool functions below deliberately mirror the names in
``mcp_ifc.tools`` (separate module namespace) so that the wire tool names
stay agent-friendly: `ifc_summary`, `ifc_query_elements`, `ifc_get_psets`,
`ifc_validate_required_props`, `ifc_export_submodel`.
"""

from __future__ import annotations

import sys

from mcp.server.fastmcp import FastMCP

from mcp_cad_core import WORKSPACE_ROOT_ENV
from mcp_cad_core.log import configure_logging, get_logger
from mcp_cad_core.sandbox import SandboxError, workspace_sandbox
from mcp_ifc import __version__
from mcp_ifc import tools as t

_logger = get_logger("mcp-ifc")

mcp = FastMCP(
    name="mcp-ifc",
    instructions=(
        "IFC/BIM inspection, query and validation tools, plus the mutating "
        "ifc_export_submodel. Start with ifc_summary to orient yourself, "
        "then ifc_query_elements to list elements, then ifc_get_psets or "
        "ifc_validate_required_props for details. All tools return JSON envelopes: "
        '{"ok": true, "data": ..., "meta": ...} on success, '
        '{"ok": false, "error": {"code": ..., "message": ...}} on failure. '
        "Paths are relative to the workspace root and sandboxed; traversal is rejected. "
        "The mutating tool always writes a new file under outputs/ and returns "
        "a summary; input files are never modified."
    ),
)


@mcp.tool()
def ifc_summary(ifc_path: str) -> dict:
    """Summarize an IFC file: schema, length unit, totals, element counts by class.

    Cheap first call for orientation. Returns a compact overview instead of
    the full element tree. Follow up with ifc_query_elements for per-class lists.

    Args:
        ifc_path: Path to the .ifc file relative to the workspace root
            (MCP_CAD_WORKSPACE_ROOT). Absolute paths inside the root also work.
    """
    return t.ifc_summary(ifc_path)


@mcp.tool()
def ifc_query_elements(
    ifc_path: str,
    ifc_class: str = "IfcProduct",
    name_pattern: str | None = None,
    limit: int = 50,
    offset: int = 0,
) -> dict:
    """List IFC elements page by page, filtered by IFC class and/or a name regex.

    Returns compact descriptors (GlobalId, class, name, predefined type,
    container) - never geometry. Use ifc_get_psets on a returned GlobalId.

    Args:
        ifc_path: Path to the .ifc file relative to the workspace root.
        ifc_class: IFC class including subclasses, e.g. IfcWall, IfcDoor,
            IfcSpace. Default: IfcProduct (all building elements).
        name_pattern: Optional case-insensitive regex matched against Name,
            e.g. "Wall.*[0-9]". Invalid regex -> invalid_input error.
        limit: Page size, 1..200 (larger values are clamped).
        offset: Zero-based index of the first returned element.
    """
    return t.ifc_query_elements(ifc_path, ifc_class, name_pattern, limit, offset)


@mcp.tool()
def ifc_get_psets(ifc_path: str, global_id: str) -> dict:
    """Return all property sets and properties of one IFC element, by GlobalId.

    Read the property data of a single element without loading geometry.
    Use ifc_query_elements first to find GlobalIds.

    Args:
        ifc_path: Path to the .ifc file relative to the workspace root.
        global_id: 22-character IFC GlobalId of the element.
    """
    return t.ifc_get_psets(ifc_path, global_id)


@mcp.tool()
def ifc_validate_required_props(
    ifc_path: str,
    ifc_class: str,
    properties: list[str],
    pset_name: str | None = None,
    limit: int = 50,
    offset: int = 0,
) -> dict:
    """Audit all elements of an IFC class for missing required properties.

    An element flags an issue when any required property is absent, null or
    empty. Returns a summary (checked, issue count, pass rate) plus
    paginated issue records. Use for model QA before downstream automation.

    Args:
        ifc_path: Path to the .ifc file relative to the workspace root.
        ifc_class: IFC class to audit including subclasses, e.g. IfcWall.
        properties: Required property names, at least one (checked across all
            property sets, or one pset when pset_name is given).
        pset_name: Optional property set scope, e.g. Pset_WallCommon.
        limit: Max issue records returned, 1..200 (clamped).
        offset: Zero-based index of the first returned issue record.
    """
    return t.ifc_validate_required_props(ifc_path, ifc_class, properties, pset_name, limit, offset)


@mcp.tool()
def ifc_export_submodel(
    ifc_path: str,
    output_name: str,
    class_filter: list[str] | None = None,
    global_ids: list[str] | None = None,
    container_global_id: str | None = None,
) -> dict:
    """Export matching IFC products to a NEW file under `<workspace>/outputs/` (mutating).

    Requires at least one selector: class_filter, global_ids (cap 200), and/or
    container_global_id. The input file is never modified; the new output file
    is validated by re-opening before ok is returned. Existing output names are
    refused (output_exists).

    Args:
        ifc_path: Path to the source .ifc file relative to the workspace root.
        output_name: Plain `.ifc` file name for the new file under `outputs/`.
        class_filter: Optional IFC classes to keep, e.g. ["IfcWall", "IfcSlab"].
        global_ids: Optional GlobalIds to keep (cap 200, clamped).
        container_global_id: Optional spatial container GlobalId; exports its
            contained elements.
    """
    return t.ifc_export_submodel(
        ifc_path, output_name, class_filter, global_ids, container_global_id
    )


def main() -> None:
    """Configure logging, fail fast on a bad workspace, and serve on stdio."""
    configure_logging()
    try:
        sandbox = workspace_sandbox()
    except SandboxError as exc:
        # Fail fast: without a valid workspace the server cannot do anything useful.
        print(f"mcp-ifc: refusing to start: {exc.message}", file=sys.stderr)
        print(
            f"Set {WORKSPACE_ROOT_ENV}=/absolute/path/to/your/ifc/workspace and retry.",
            file=sys.stderr,
        )
        raise SystemExit(2) from None
    _logger.info("mcp-ifc %s starting (workspace=%s, transport=stdio)", __version__, sandbox.root)
    mcp.run(transport="stdio")


if __name__ == "__main__":
    main()
