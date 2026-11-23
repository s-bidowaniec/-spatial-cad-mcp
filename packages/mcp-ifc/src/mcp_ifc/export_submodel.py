"""Submodel export for ``ifc_export_submodel`` (first IFC mutating tool).

Strategy: copy the source IFC file to a temporary sibling of the output, open
the copy, remove the non-matching products (``IfcProduct`` except
``IfcSpatialElement``, so the spatial hierarchy, header, type objects and
materials survive), write the result to the sandboxed output path, and
re-open the written file to validate it before returning ``ok``. The input
file is never modified.
"""

from __future__ import annotations

import contextlib
import shutil
from collections import Counter
from pathlib import Path
from typing import Any

import ifcopenshell
import ifcopenshell.util.element

from mcp_cad_core.responses import ToolError
from mcp_ifc.model_cache import load_ifc

#: Hard cap on explicitly requested GlobalIds per export call.
MAX_GLOBAL_IDS = 200


def validate_output_name(output_name: str) -> str:
    """Return the cleaned output file name or raise ``invalid_input``.

    The name must be a plain ``.ifc`` file name: no path separators, no
    ``..``, and it will land under ``<workspace>/outputs/``.
    """
    name = (output_name or "").strip()
    if not name:
        raise ToolError("invalid_input", "output_name must be a non-empty .ifc file name.")
    if "/" in name or "\\" in name or ".." in name or name in (".", ".."):
        raise ToolError(
            "invalid_input",
            "output_name must be a plain file name without path separators or '..'.",
        )
    if not name.lower().endswith(".ifc"):
        raise ToolError("invalid_input", "output_name must end with '.ifc'.")
    return name


def _collect_contained_ids(spatial_entity: Any) -> set[str]:
    """GlobalIds of products contained directly in ``spatial_entity``."""
    ids: set[str] = set()
    for rel in getattr(spatial_entity, "ContainsElements", []) or []:
        for elem in getattr(rel, "RelatedElements", []) or []:
            gid = getattr(elem, "GlobalId", None)
            if gid:
                ids.add(str(gid))
    return ids


def _remove_product(model: Any, product: Any) -> None:
    """Remove one product and its subgraph, detaching relationship inverses first.

    ``remove_deep2`` refuses to delete an element that still has inverses, so
    every ``IfcRelationship`` referencing the product is fixed up first: list
    attributes (e.g. ``RelatedElements``) are filtered, relationships that
    exist solely for the product (single ``Relating*`` reference) are removed.
    """
    pid = product.id()
    for rel in list(model.by_type("IfcRelationship")):
        for attr in [n for n in dir(rel) if n[:1].isupper()]:
            try:
                val = getattr(rel, attr)
            except Exception:
                continue
            if isinstance(val, list | tuple):
                kept = [v for v in val if not (hasattr(v, "id") and v.id() == pid)]
                if len(kept) != len(val):
                    if kept:
                        setattr(rel, attr, kept)
                    else:
                        model.remove(rel)
                    break
            elif hasattr(val, "id") and val.id() == pid:
                model.remove(rel)
                break
    ifcopenshell.util.element.remove_deep2(model, product)


def export_submodel(
    input_path: Path,
    output_path: Path,
    *,
    class_filter: list[str] | None = None,
    global_ids: list[str] | None = None,
    container_global_id: str | None = None,
) -> dict[str, Any]:
    """Write the selected products of ``input_path`` to ``output_path`` as a valid IFC file.

    Selectors combine by intersection when several are given: an element must
    match every provided selector. Unknown GlobalIds (and a missing container)
    do not abort the export; they are reported in ``missing_global_ids``.
    Returns the ``data`` payload (output, elements_before/after, by_class,
    file_size_bytes, missing_global_ids).
    """
    classes = [c for c in (class_filter or []) if c]
    raw_gids = [g for g in (global_ids or []) if g]
    container_gid = container_global_id or None

    source_model = load_ifc(input_path)
    for cls in classes:
        try:
            source_model.by_type(cls)
        except Exception as exc:
            raise ToolError(
                "invalid_input",
                f"Unknown IFC class '{cls}'. Check spelling/casing, e.g. IfcWall, IfcSlab.",
                details={"reason": str(exc)[:200]},
            ) from exc

    all_products = list(source_model.by_type("IfcProduct"))
    elements_before = len(all_products)
    product_by_guid = {
        str(getattr(p, "GlobalId", "")): p for p in all_products if getattr(p, "GlobalId", None)
    }

    missing_global_ids: list[str] = []
    gid_set: set[str] | None = None
    if raw_gids:
        gid_set = set()
        for gid in raw_gids:
            if gid in product_by_guid:
                gid_set.add(gid)
            else:
                missing_global_ids.append(gid)

    container_ids: set[str] | None = None
    if container_gid:
        try:
            container_obj = source_model.by_guid(container_gid)
        except Exception:
            container_obj = None
        if container_obj is None:
            missing_global_ids.append(container_gid)
            container_ids = set()
        else:
            container_ids = _collect_contained_ids(container_obj)

    class_set = set(classes)
    selected_guids: set[str] = set()
    for prod in all_products:
        gid = str(getattr(prod, "GlobalId", "") or "")
        if class_set and not any(prod.is_a(c) for c in class_set):
            continue
        if gid_set is not None and gid not in gid_set:
            continue
        if container_ids is not None and gid not in container_ids:
            continue
        selected_guids.add(gid)

    tmp_path = output_path.parent / (output_path.name + ".tmp")
    try:
        output_path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(input_path, tmp_path)
        sub_model = ifcopenshell.open(str(tmp_path))
        for prod in list(sub_model.by_type("IfcProduct")):
            if prod.is_a("IfcSpatialElement"):
                continue
            if str(getattr(prod, "GlobalId", "") or "") not in selected_guids:
                _remove_product(sub_model, prod)
        sub_model.write(str(output_path))
        validated = ifcopenshell.open(str(output_path))
    except Exception as exc:
        with contextlib.suppress(OSError):
            output_path.unlink(missing_ok=True)
        raise ToolError(
            "internal_error",
            f"Submodel export or validation failed: {exc}",
            retryable=True,
        ) from exc
    finally:
        tmp_path.unlink(missing_ok=True)

    final_products = [p for p in validated.by_type("IfcProduct") if not p.is_a("IfcSpatialElement")]
    by_class = dict(sorted(Counter(p.is_a() for p in final_products).items()))
    data: dict[str, Any] = {
        "output": f"outputs/{output_path.name}",
        "elements_before": elements_before,
        "elements_after": len(final_products),
        "by_class": by_class,
        "file_size_bytes": output_path.stat().st_size,
    }
    if missing_global_ids:
        data["missing_global_ids"] = missing_global_ids
    return data


__all__ = ["MAX_GLOBAL_IDS", "export_submodel", "validate_output_name"]
