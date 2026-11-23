"""MCP tool implementations for mcp-ifc.

Every tool is:

- read-only (no file inside the workspace is modified),
- sandboxed (paths resolved via ``mcp_cad_core.workspace_sandbox``),
- envelope-shaped (see ``mcp_cad_core.responses``),
- compact (summaries and paginated slices, never full geometry trees).

Tool functions are plain Python functions so they are trivially testable;
``server.py`` registers them on the FastMCP instance.
"""

from __future__ import annotations

import re
from collections import Counter
from pathlib import Path
from typing import Any

import ifcopenshell
import ifcopenshell.util.element

from mcp_cad_core.log import get_logger
from mcp_cad_core.pagination import Page, PaginationError, paginate, truncate_value
from mcp_cad_core.responses import ToolError, ok, safe_tool
from mcp_cad_core.sandbox import workspace_sandbox
from mcp_ifc.geometry_summary import element_bbox_summary, geometry_summary
from mcp_ifc.model_cache import load_ifc, shape_summary_cache

_logger = get_logger(__name__)


def _resolve_input_path(ifc_path: str) -> Path:
    """Sandbox-resolve ``ifc_path`` to an existing file inside the workspace."""
    return workspace_sandbox().resolve(ifc_path, must_exist=True, expect=("file",))


def _resolve_input(ifc_path: str) -> ifcopenshell.file:
    """Sandbox-resolve ``ifc_path`` and return the parsed model."""
    return load_ifc(_resolve_input_path(ifc_path))


def _shape_file_key(path: Path) -> str:
    """Cache key for shape summaries: absolute path plus stat, so edits invalidate."""
    stat = path.stat()
    return f"{path}|{stat.st_mtime_ns}|{stat.st_size}"


def _paged(items: list[Any], limit: int, offset: int) -> Page:
    """Paginate with a deterministic invalid-input mapping."""
    try:
        return paginate(items, limit=limit, offset=offset)
    except PaginationError as exc:
        raise ToolError("invalid_input", str(exc)) from exc


def _compact_element(element: ifcopenshell.entity_instance) -> dict[str, Any]:
    """One-line JSON description of an element (no geometry)."""
    predefined_type = getattr(element, "PredefinedType", None)
    container = ifcopenshell.util.element.get_container(element)
    return {
        "global_id": getattr(element, "GlobalId", None),
        "ifc_class": element.is_a(),
        "name": truncate_value(getattr(element, "Name", None), 120) or None,
        "predefined_type": str(predefined_type) if predefined_type is not None else None,
        "container": (
            container.Name if container is not None and getattr(container, "Name", None) else None
        ),
        "element_id": element.id(),
    }


@safe_tool
def ifc_summary(ifc_path: str) -> dict[str, Any]:
    """Summarize an IFC file: schema, length units, and element counts by class.

    Cheap first call for orientation. Returns a compact overview instead of
    the full element tree. Follow up with `ifc_query_elements` to list
    elements of a specific class.

    Args:
        ifc_path: Path to the .ifc file, relative to the workspace root
            (MCP_CAD_WORKSPACE_ROOT). Absolute paths inside the root also work.

    Returns:
        Envelope with data: model name, schema version (e.g. IFC4), length
        unit, total entity count, total product count, per-class product
        counts (top 25 by frequency).
    """
    model = _resolve_input(ifc_path)
    counts = Counter(e.is_a() for e in model.by_type("IfcProduct"))
    projects = model.by_type("IfcProject")
    data: dict[str, Any] = {
        "ifc_path": ifc_path,
        "file_name": _header_file_name(model),
        "project_name": truncate_value(projects[0].Name, 200) if projects else None,
        "schema": model.schema,
        "length_unit": _detect_length_unit(model),
        "total_entities": len(list(model)),
        "total_products": sum(counts.values()),
        "class_counts": [
            {"ifc_class": cls, "count": count} for cls, count in counts.most_common(25)
        ],
    }
    return ok(data)


def _header_file_name(model: ifcopenshell.file) -> str | None:
    """STEP header FILE_NAME, or None when unavailable (cosmetic field only).

    ifcopenshell 0.8 exposes it as ``wrapped_data.header().file_name_py().name``
    (both ``header`` and ``file_name_py`` are methods there).
    """
    try:
        header = model.wrapped_data.header
        header = header() if callable(header) else header
        if header is None:
            return None
        file_name = getattr(header, "file_name_py", None) or getattr(header, "file_name", None)
        if file_name is None:
            return None
        file_name = file_name() if callable(file_name) else file_name
        name = getattr(file_name, "name", None)
        return str(name) if name else None
    except Exception:
        return None


def _detect_length_unit(model: ifcopenshell.file) -> str | None:
    """Find the project's length unit as a human-readable string (e.g. MILLIMETRE)."""
    for assignment in model.by_type("IfcUnitAssignment"):
        for unit in assignment.Units or []:
            if unit.is_a("IfcSIUnit") and str(unit.UnitType) == "LENGTHUNIT":
                prefix = unit.Prefix
                prefix_s = str(prefix).title() if prefix is not None else ""
                return f"{prefix_s}{unit.Name}"
            if unit.is_a("IfcConversionBasedUnit") and str(unit.UnitType) == "LENGTHUNIT":
                return str(unit.Name)
    return None


@safe_tool
def ifc_query_elements(
    ifc_path: str,
    ifc_class: str = "IfcProduct",
    name_pattern: str | None = None,
    limit: int = 50,
    offset: int = 0,
) -> dict[str, Any]:
    """List IFC elements page by page, optionally filtered by class and name.

    Returns only compact element descriptors (GlobalId, class, name,
    predefined type, spatial container) - never geometry. Use
    `ifc_get_psets` on a returned GlobalId for property details.

    Args:
        ifc_path: Path to the .ifc file, relative to the workspace root.
        ifc_class: IFC class to list, including subclasses (e.g. IfcWall,
            IfcDoor, IfcSpace). Defaults to IfcProduct (all building elements).
        name_pattern: Optional case-insensitive regular expression matched
            against element Name (e.g. "Wall.*[0-9]"). Invalid regexes are
            reported as invalid_input errors.
        limit: Page size (1..200; values above 200 are clamped).
        offset: Zero-based index of the first returned element.

    Returns:
        Envelope with data.elements (list) plus pagination metadata
        (total, limit, offset, has_more, next_offset).
    """
    model = _resolve_input(ifc_path)
    if not ifc_class or not ifc_class.strip():
        raise ToolError(
            "invalid_input", "ifc_class must be a non-empty IFC class name, e.g. IfcWall."
        )
    class_name = ifc_class.strip()

    pattern: re.Pattern[str] | None = None
    if name_pattern is not None and name_pattern != "":
        try:
            pattern = re.compile(name_pattern, re.IGNORECASE)
        except re.error as exc:
            raise ToolError(
                "invalid_input",
                f"name_pattern is not a valid regular expression: {exc}",
            ) from exc

    try:
        elements = model.by_type(class_name)
    except Exception as exc:
        raise ToolError(
            "invalid_input",
            f"Unknown IFC class '{class_name}'. Check spelling and casing, e.g. IfcWall, IfcSlab.",
            details={"reason": str(exc)[:200]},
        ) from exc

    if pattern is not None:
        elements = [e for e in elements if pattern.search(getattr(e, "Name", None) or "")]
    page = _paged([_compact_element(e) for e in elements], limit, offset)
    data = {
        "ifc_path": ifc_path,
        "query": {"ifc_class": class_name, "name_pattern": name_pattern or None},
        "elements": page.items,
    }
    return ok(data, meta=page.meta())


@safe_tool
def ifc_get_psets(ifc_path: str, global_id: str) -> dict[str, Any]:
    """Return all property sets and properties of one IFC element, by GlobalId.

    Read the property data (Psets) of a single element without loading
    geometry. Use `ifc_query_elements` first to find GlobalIds.

    Args:
        ifc_path: Path to the .ifc file, relative to the workspace root.
        global_id: The IFC GlobalId (22-char base64 string, e.g.
            "3vYgWfk3j5JPz8MdE8r9QX") of the element to inspect.

    Returns:
        Envelope with data.psets: mapping of property set name to
        property->value mapping, plus counts. Long values are truncated
        (see meta for notes).
    """
    model = _resolve_input(ifc_path)
    guid = (global_id or "").strip()
    if not guid:
        raise ToolError("invalid_input", "global_id must be a 22-character IFC GlobalId.")

    try:
        element = model.by_guid(guid)
    except Exception as exc:
        raise ToolError(
            "element_not_found",
            f"No element with GlobalId '{guid}' exists in this model. "
            "Call ifc_query_elements to list valid GlobalIds.",
            details={"global_id": guid},
        ) from exc

    raw_psets: dict[str, dict[str, Any]] = ifcopenshell.util.element.get_psets(element)
    psets: dict[str, dict[str, Any]] = {}
    property_count = 0
    truncated = 0
    for pset_name, props in raw_psets.items():
        clean: dict[str, Any] = {}
        for prop_name, value in props.items():
            if prop_name == "id":  # internal ifcopenshell entity id, not property data
                continue
            compact = truncate_value(value)
            if isinstance(compact, str) and compact != value:
                truncated += 1
            clean[prop_name] = compact
            property_count += 1
        psets[pset_name] = clean

    data = {
        "global_id": guid,
        "ifc_class": element.is_a(),
        "name": truncate_value(getattr(element, "Name", None), 120) or None,
        "pset_count": len(psets),
        "property_count": property_count,
        "psets": psets,
    }
    meta: dict[str, Any] = {}
    if truncated:
        meta["truncated_values"] = truncated
    return ok(data, meta=meta)


@safe_tool
def ifc_validate_required_props(
    ifc_path: str,
    ifc_class: str,
    properties: list[str],
    pset_name: str | None = None,
    limit: int = 50,
    offset: int = 0,
) -> dict[str, Any]:
    """Audit elements of an IFC class for missing required properties.

    Answers "which elements are incomplete?" deterministically: an element
    flags an issue when any of the required property names is absent or set
    to null/empty across the considered property sets. Use for model QA and
    data-completeness checks before downstream automation.

    Args:
        ifc_path: Path to the .ifc file, relative to the workspace root.
        ifc_class: IFC class to audit, including subclasses (e.g. IfcWall).
        properties: Property names that must be present and non-empty
            (at least one). Order is preserved in the report.
        pset_name: Optional property set name restricting the audit scope
            (case-insensitive, e.g. Pset_WallCommon). When omitted, every
            property set on the element is considered.
        limit: Maximum number of issue records returned (1..200).
        offset: Zero-based index of the first returned issue record.

    Returns:
        Envelope with data.summary (checked count, issue count, pass rate)
        and data.issues (paginated records with global_id, element name,
        and missing property names).
    """
    model = _resolve_input(ifc_path)
    class_name = (ifc_class or "").strip()
    required = [p for p in (properties or []) if p and p.strip()]
    if not class_name:
        raise ToolError(
            "invalid_input", "ifc_class must be a non-empty IFC class name, e.g. IfcWall."
        )
    if not required:
        raise ToolError(
            "invalid_input", "properties must contain at least one property name to check."
        )

    try:
        elements = model.by_type(class_name)
    except Exception as exc:
        raise ToolError(
            "invalid_input",
            f"Unknown IFC class '{class_name}'. Check spelling and casing, e.g. IfcWall, IfcSlab.",
            details={"reason": str(exc)[:200]},
        ) from exc

    scoped_pset: str | None = pset_name.strip().upper() if pset_name else None
    issues: list[dict[str, Any]] = []
    for element in elements:
        raw_psets: dict[str, dict[str, Any]] = ifcopenshell.util.element.get_psets(element)
        considered = {
            name: props
            for name, props in raw_psets.items()
            if scoped_pset is None or name.upper() == scoped_pset
        }
        missing = _missing_required(required, considered)
        if missing:
            issues.append(
                {
                    "global_id": getattr(element, "GlobalId", None),
                    "ifc_class": element.is_a(),
                    "name": truncate_value(getattr(element, "Name", None), 120) or None,
                    "missing": missing,
                    "pset_names_checked": list(considered),
                }
            )

    page = _paged(issues, limit, offset)
    checked = len(elements)
    issue_count = len(issues)
    data = {
        "ifc_path": ifc_path,
        "audit": {
            "ifc_class": class_name,
            "pset_name": pset_name or None,
            "required_properties": required,
        },
        "summary": {
            "checked": checked,
            "elements_with_issues": issue_count,
            "elements_passing": checked - issue_count,
            "pass_rate": round((checked - issue_count) / checked, 4) if checked else None,
        },
        "issues": page.items,
    }
    return ok(data, meta=page.meta())


def _missing_required(required: list[str], psets: dict[str, dict[str, Any]]) -> list[str]:
    """Return required properties that are absent, null or empty across all given psets."""
    missing: list[str] = []
    for prop in required:
        values = [v for props in psets.values() for name, v in props.items() if name == prop]
        has_value = any(v is not None and v != "" for v in values)
        if not has_value:
            missing.append(prop)
    return missing


@safe_tool
def ifc_element_bbox(
    ifc_path: str, global_id: str, include_children: bool = False
) -> dict[str, Any]:
    """Compute the 3D world bounding box, center, and triangle count of one IFC element.

    Read-only geometry summary via the OCCT kernel (`ifcopenshell.geom`): returns
    only bbox min/max, center, and `geometry_tris` - never vertex arrays or mesh
    buffers. With `include_children` on a spatial element, the bbox is the union
    of its contained elements (capped at 200 children). When the geometry kernel
    is not installed the tool answers `not_implemented` instead of crashing.

    Args:
        ifc_path: Path to the .ifc file, relative to the workspace root.
        global_id: 22-character IFC GlobalId of the element.
        include_children: Union the bboxes of contained elements (spatial
            containers), cap 200 children.

    Returns:
        Envelope with data: global_id, class, bbox {min, max}, center,
        geometry_tris (and children_count when include_children is set).
    """
    path = _resolve_input_path(ifc_path)
    model = load_ifc(path)
    data = element_bbox_summary(
        model,
        shape_summary_cache(),
        _shape_file_key(path),
        global_id,
        include_children=include_children,
    )
    data["ifc_path"] = ifc_path
    return ok(data)


@safe_tool
def ifc_geometry_summary(
    ifc_path: str,
    class_filter: str | None = None,
    limit: int = 50,
    offset: int = 0,
) -> dict[str, Any]:
    """Per-class geometry aggregates: count, bbox union, and smallest-footprint element.

    Read-only rollup over all products (or one IFC class): for each class the
    element count, the union bbox, and the element with the smallest bbox
    footprint (`bbox_min_area_element` with global_id and name). Paginated,
    hard cap 200 classes, and never any raw geometry - only bboxes and counts.
    Requires the OCCT kernel (`ifcopenshell.geom`); otherwise `not_implemented`.

    Args:
        ifc_path: Path to the .ifc file, relative to the workspace root.
        class_filter: Optional IFC class to restrict the rollup, e.g. IfcWall.
        limit: Page size (1..200; values above 200 are clamped).
        offset: Zero-based index of the first returned class row.

    Returns:
        Envelope with data.classes (paginated) plus pagination metadata.
    """
    path = _resolve_input_path(ifc_path)
    model = load_ifc(path)
    data, page = geometry_summary(
        model,
        shape_summary_cache(),
        _shape_file_key(path),
        class_filter=class_filter,
        limit=limit,
        offset=offset,
    )
    data["ifc_path"] = ifc_path
    return ok(data, meta=page.meta())
