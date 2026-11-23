"""Spatial hierarchy browsing helpers for ``ifc_query_spatial``.

Pure functions over an ``ifcopenshell`` model: walk ``IfcRelAggregates``
(``IsDecomposedBy``) from the root and count contained elements via
``IfcRelContainedInSpatialStructure``. Never return element dumps - only
counts and compact tree rows.
"""

from __future__ import annotations

from typing import Any

from mcp_cad_core.pagination import MAX_LIMIT, paginate
from mcp_cad_core.responses import ToolError

MAX_SPATIAL_DEPTH = 10
DEFAULT_SPATIAL_DEPTH = 3


def _contained_element_ids(spatial_entity: Any) -> set[str]:
    """GlobalIds of product elements contained directly in ``spatial_entity``."""
    contained: set[str] = set()
    for rel in getattr(spatial_entity, "ContainsElements", []) or []:
        for elem in getattr(rel, "RelatedElements", []) or []:
            gid = getattr(elem, "GlobalId", None)
            if gid:
                contained.add(str(gid))
    return contained


def _decomposed_children(spatial_entity: Any) -> list[Any]:
    """Child spatial elements linked via ``IsDecomposedBy`` (IfcRelAggregates)."""
    children: list[Any] = []
    seen: set[int] = set()
    for rel in getattr(spatial_entity, "IsDecomposedBy", []) or []:
        for child in getattr(rel, "RelatedObjects", []) or []:
            if (
                getattr(child, "is_a", None)
                and child.is_a("IfcSpatialElement")
                and id(child) not in seen
            ):
                seen.add(id(child))
                children.append(child)
    return children


def _resolve_root(model: Any, root_global_id: str | None) -> Any:
    """Resolve the starting spatial entity by GlobalId or the project/site root."""
    if root_global_id is not None and root_global_id.strip():
        target = root_global_id.strip()
        try:
            entity = model.by_guid(target)
        except Exception:
            entity = None
        if entity is None:
            raise ToolError(
                "element_not_found",
                f"Spatial element with GlobalId '{target}' not found in this model.",
                hints=["Call ifc_query_spatial without root_global_id to list the tree."],
            )
        return entity
    for cls in ("IfcSite", "IfcProject", "IfcBuilding"):
        found = model.by_type(cls)
        if found:
            return found[0]
    raise ToolError("cannot_open_ifc", "No spatial hierarchy root found in IFC model.")


def query_spatial_hierarchy(
    model: Any,
    root_global_id: str | None = None,
    depth: int | None = None,
    limit: int | None = None,
    offset: int = 0,
) -> tuple[dict[str, Any], Any]:
    """Flatten the spatial tree into rows, deterministically ordered.

    Returns ``(data, page)``; use ``page.meta()`` for the envelope meta.
    """
    max_depth = DEFAULT_SPATIAL_DEPTH if depth is None else depth
    if not isinstance(max_depth, int) or max_depth < 1 or max_depth > MAX_SPATIAL_DEPTH:
        raise ToolError(
            "invalid_input",
            f"depth must be an integer between 1 and {MAX_SPATIAL_DEPTH}, got {max_depth!r}.",
        )

    root = _resolve_root(model, root_global_id)

    rows: list[dict[str, Any]] = []
    queue: list[tuple[Any, int, str | None]] = [(root, 0, None)]
    visited: set[str] = set()
    while queue:
        entity, level, parent_gid = queue.pop(0)
        gid = str(getattr(entity, "GlobalId", "") or "")
        if gid in visited:
            continue
        visited.add(gid)
        rows.append(
            {
                "global_id": gid,
                "class": entity.is_a(),
                "name": getattr(entity, "Name", None) or "",
                "level": level,
                "parent_global_id": parent_gid,
                "contained_elements": len(_contained_element_ids(entity)),
            }
        )
        if level < max_depth:
            for child in _decomposed_children(entity):
                child_gid = str(getattr(child, "GlobalId", "") or "")
                if child_gid not in visited:
                    queue.append((child, level + 1, gid or None))

    rows.sort(key=lambda r: (r["level"], r["class"], r["name"], r["global_id"]))
    page = _paginate_rows(rows, limit, offset)
    data = {
        "root_global_id": str(getattr(root, "GlobalId", "") or ""),
        "depth": max_depth,
        "rows": page.items,
    }
    return data, page


def _paginate_rows(rows: list[dict[str, Any]], limit: int | None, offset: int) -> Any:
    from mcp_cad_core.pagination import PaginationError

    try:
        return paginate(rows, limit=limit or 50, offset=offset)
    except PaginationError as exc:
        raise ToolError("invalid_input", str(exc)) from exc


def filter_elements_by_container(
    model: Any,
    elements: list[Any],
    container_global_id: str | None,
) -> list[Any]:
    """Keep only elements contained in the spatial container ``container_global_id``."""
    if not container_global_id or not container_global_id.strip():
        return elements
    target = container_global_id.strip()
    try:
        container = model.by_guid(target)
    except Exception:
        container = None
    if container is None:
        raise ToolError(
            "element_not_found",
            f"Container '{target}' not found in IFC model.",
            hints=["Call ifc_query_spatial to list spatial containers and their GlobalIds."],
        )
    contained = _contained_element_ids(container)
    return [el for el in elements if str(getattr(el, "GlobalId", "") or "") in contained]


__all__ = [
    "DEFAULT_SPATIAL_DEPTH",
    "MAX_LIMIT",
    "MAX_SPATIAL_DEPTH",
    "filter_elements_by_container",
    "query_spatial_hierarchy",
]
