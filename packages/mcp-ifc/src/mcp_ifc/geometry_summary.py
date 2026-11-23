"""Geometry-level summaries for mcp-ifc: per-element bboxes and per-class aggregates.

Read-only and context-budget safe: results never contain raw geometry (no
vertex arrays, no mesh buffers) - only bounding boxes, centers, triangle
counts and per-class aggregates. Requires the OCCT geometry kernel
(``ifcopenshell.geom``, shipped with the ``ifcopenshell[occ]`` extra); when the
kernel is unavailable the tools answer with a ``not_implemented`` envelope
instead of crashing. Computed shape summaries are cached per
(file, GlobalId) with a TTL (see ``model_cache.ShapeSummaryTtlCache``).
"""

from __future__ import annotations

import importlib
from typing import Any

import ifcopenshell

from mcp_cad_core.pagination import DEFAULT_LIMIT, PaginationError, paginate
from mcp_cad_core.responses import ToolError

#: Hard cap on children unioned into one `ifc_element_bbox` call.
MAX_CHILDREN_UNION = 200

GEOM_KERNEL_UNAVAILABLE = "geometry kernel unavailable; install ifcopenshell[occ]"


def load_geom_kernel() -> Any | None:
    """Return the ``ifcopenshell.geom`` module, or None when the OCCT kernel is missing."""
    try:
        geom = getattr(ifcopenshell, "geom", None)
        if geom is None:
            geom = importlib.import_module("ifcopenshell.geom")
        return geom
    except Exception:
        return None


def require_geom_kernel() -> Any:
    """Return the geometry kernel or raise a deterministic ``not_implemented`` error."""
    geom = load_geom_kernel()
    if geom is None:
        raise ToolError(
            "not_implemented",
            GEOM_KERNEL_UNAVAILABLE,
            hints=["Install the OCCT-based kernel, e.g. pip install 'ifcopenshell[occ]'."],
        )
    return geom


def _compute_shape_summary(geom: Any, product: Any) -> dict[str, Any]:
    """Triangulate one product in world coords; return bbox + triangle count only."""
    settings = geom.settings()
    settings.set(settings.USE_WORLD_COORDS, True)
    triangulated = getattr(settings, "TRIANGULATED", None)
    if triangulated is not None:
        settings.set(triangulated, True)
    shape = geom.create_shape(settings, product)
    geometry = shape.geometry
    verts = [float(v) for v in geometry.verts]
    faces = list(geometry.faces)
    if not verts:
        return null_summary()
    xs, ys, zs = verts[0::3], verts[1::3], verts[2::3]
    return {
        "bbox_min": [min(xs), min(ys), min(zs)],
        "bbox_max": [max(xs), max(ys), max(zs)],
        "tris": len(faces) // 3,
    }


def null_summary() -> dict[str, Any]:
    return {"bbox_min": None, "bbox_max": None, "tris": 0}


def element_shape_summary(
    geom: Any, shape_cache: Any, file_key: str, element: Any
) -> dict[str, Any] | None:
    """Cached per-element shape summary, or None when the element has no geometry."""
    gid = str(getattr(element, "GlobalId", "") or "")
    if not gid or getattr(element, "Representation", None) is None:
        return None
    cached = shape_cache.get(file_key, gid)
    if cached is not None:
        return cached
    summary = _compute_shape_summary(geom, element)
    shape_cache.set(file_key, gid, summary)
    return summary


def _contained_elements(spatial_entity: Any) -> list[Any]:
    """Products contained directly in ``spatial_entity`` via IfcRelContainedInSpatialStructure."""
    contained: list[Any] = []
    seen: set[int] = set()
    for rel in getattr(spatial_entity, "ContainsElements", []) or []:
        for elem in getattr(rel, "RelatedElements", []) or []:
            if id(elem) not in seen:
                seen.add(id(elem))
                contained.append(elem)
    return contained


def _find_element(model: Any, global_id: str) -> Any:
    guid = (global_id or "").strip()
    if not guid:
        raise ToolError("invalid_input", "global_id must be a 22-character IFC GlobalId.")
    try:
        return model.by_guid(guid)
    except Exception as exc:
        raise ToolError(
            "element_not_found",
            f"No element with GlobalId '{guid}' exists in this model. "
            "Call ifc_query_elements to list valid GlobalIds.",
            details={"global_id": guid},
        ) from exc


def element_bbox_summary(
    model: Any,
    shape_cache: Any,
    file_key: str,
    global_id: str,
    include_children: bool = False,
) -> dict[str, Any]:
    """bbox/center/triangle-count for one element (optionally unioned over children)."""
    geom = require_geom_kernel()
    element = _find_element(model, global_id)
    guid = str(getattr(element, "GlobalId", "") or "")

    targets: list[Any] = [element]
    children_clamped_to: int | None = None
    if include_children:
        contained = _contained_elements(element)
        if len(contained) > MAX_CHILDREN_UNION:
            contained = contained[:MAX_CHILDREN_UNION]
            children_clamped_to = MAX_CHILDREN_UNION
        targets = contained or [element]

    summaries = [
        s
        for s in (element_shape_summary(geom, shape_cache, file_key, t) for t in targets)
        if s is not None and s["bbox_min"] is not None
    ]
    if not summaries:
        raise ToolError(
            "no_geometry",
            f"Element '{guid}' has no triangulated geometry (missing or empty representation).",
            details={"global_id": guid},
        )

    bbox_min = [min(s["bbox_min"][i] for s in summaries) for i in range(3)]
    bbox_max = [max(s["bbox_max"][i] for s in summaries) for i in range(3)]
    data: dict[str, Any] = {
        "global_id": guid,
        "class": element.is_a(),
        "bbox": {"min": bbox_min, "max": bbox_max},
        "center": [(a + b) / 2.0 for a, b in zip(bbox_min, bbox_max, strict=True)],
        "geometry_tris": sum(s["tris"] for s in summaries),
    }
    if include_children:
        data["include_children"] = True
        data["children_count"] = len(targets)
        if children_clamped_to is not None:
            data["children_clamped_to"] = children_clamped_to
    return data


def geometry_summary(
    model: Any,
    shape_cache: Any,
    file_key: str,
    class_filter: str | None = None,
    limit: int | None = None,
    offset: int = 0,
) -> tuple[dict[str, Any], Any]:
    """Per-class aggregates (count, bbox union, min-footprint element), paginated.

    Returns ``(data, page)``; use ``page.meta()`` for the envelope meta.
    """
    geom = require_geom_kernel()
    class_name: str | None = None
    if class_filter is not None and class_filter.strip():
        class_name = class_filter.strip()
        try:
            elements = list(model.by_type(class_name))
        except Exception as exc:
            raise ToolError(
                "invalid_input",
                f"Unknown IFC class '{class_name}'. Check spelling/casing, e.g. IfcWall, IfcSlab.",
                details={"reason": str(exc)[:200]},
            ) from exc
    else:
        elements = list(model.by_type("IfcProduct"))

    aggregates: dict[str, dict[str, Any]] = {}
    for element in elements:
        summary = element_shape_summary(geom, shape_cache, file_key, element)
        if summary is None or summary["bbox_min"] is None:
            continue
        cls = element.is_a()
        agg = aggregates.setdefault(
            cls,
            {
                "class": cls,
                "count": 0,
                "bbox_min": list(summary["bbox_min"]),
                "bbox_max": list(summary["bbox_max"]),
                "min_area": None,
                "min_area_element": None,
            },
        )
        agg["count"] += 1
        for i in range(3):
            agg["bbox_min"][i] = min(agg["bbox_min"][i], summary["bbox_min"][i])
            agg["bbox_max"][i] = max(agg["bbox_max"][i], summary["bbox_max"][i])
        dx = summary["bbox_max"][0] - summary["bbox_min"][0]
        dy = summary["bbox_max"][1] - summary["bbox_min"][1]
        area = dx * dy
        if agg["min_area"] is None or area < agg["min_area"]:
            agg["min_area"] = area
            agg["min_area_element"] = {
                "global_id": str(getattr(element, "GlobalId", "") or ""),
                "name": getattr(element, "Name", None) or None,
            }

    rows = [
        {
            "class": aggregates[cls]["class"],
            "count": aggregates[cls]["count"],
            "bbox_union": {"min": aggregates[cls]["bbox_min"], "max": aggregates[cls]["bbox_max"]},
            "bbox_min_area_element": aggregates[cls]["min_area_element"],
        }
        for cls in sorted(aggregates)
    ]
    page = _paginate_rows(rows, limit, offset)
    data = {
        "class_filter": class_name,
        "elements_scanned": len(elements),
        "classes": page.items,
    }
    return data, page


def _paginate_rows(rows: list[dict[str, Any]], limit: int | None, offset: int) -> Any:
    try:
        return paginate(rows, limit=DEFAULT_LIMIT if limit is None else limit, offset=offset)
    except PaginationError as exc:
        raise ToolError("invalid_input", str(exc)) from exc


__all__ = [
    "GEOM_KERNEL_UNAVAILABLE",
    "MAX_CHILDREN_UNION",
    "element_bbox_summary",
    "geometry_summary",
    "load_geom_kernel",
    "require_geom_kernel",
]
