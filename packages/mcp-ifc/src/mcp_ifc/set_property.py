"""Property editing mutator for ``ifc_set_property``.

Strategy: copy the source IFC to a temporary sibling of the output, open the
copy, find-or-create the named ``IfcPropertySet`` on each selected element and
set one ``IfcPropertySingleValue`` (numeric strings become ``IfcInteger`` /
``IfcReal``, everything else ``IfcText`` with truncation), write the result to
the sandboxed output path, and re-open it to validate before returning ``ok``.
The input file is never modified.
"""

from __future__ import annotations

import contextlib
import shutil
from pathlib import Path
from typing import Any

import ifcopenshell
import ifcopenshell.guid

from mcp_cad_core.responses import ToolError
from mcp_ifc.model_cache import load_ifc

#: Hard cap on explicitly requested GlobalIds per call.
MAX_GLOBAL_IDS = 200

#: Hard cap on change records reported in ``data.changes``.
MAX_CHANGES_REPORTED = 200

#: Max length of a reported/stored property value string.
MAX_PROP_VALUE_CHARS = 256


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


def _truncate_value(val: str) -> str:
    cleaned = val.replace("\r", " ").replace("\n", " ")
    if len(cleaned) <= MAX_PROP_VALUE_CHARS:
        return cleaned
    marker = "...[truncated]"
    keep = max(0, MAX_PROP_VALUE_CHARS - len(marker))
    return f"{cleaned[:keep]}{marker}"


def _parse_nominal_value(model: Any, raw_value: str) -> Any:
    """Parse a numeric string into IfcInteger/IfcReal when cleanly parseable, else IfcText."""
    text = raw_value.strip()
    if text:
        try:
            return model.create_entity("IfcInteger", int(text))
        except ValueError:
            pass
        try:
            float_val = float(text)
            if float_val == float_val and abs(float_val) != float("inf"):
                return model.create_entity("IfcReal", float_val)
        except ValueError:
            pass
    return model.create_entity("IfcText", _truncate_value(raw_value))


def _get_or_create_pset(model: Any, element: Any, pset_name: str) -> Any:
    """Find the ``IfcPropertySet`` named ``pset_name`` attached to ``element``, or create one."""
    for rel in getattr(element, "IsDefinedBy", []) or []:
        if rel.is_a("IfcRelDefinesByProperties"):
            prop_def = getattr(rel, "RelatingPropertyDefinition", None)
            if (
                prop_def is not None
                and prop_def.is_a("IfcPropertySet")
                and (getattr(prop_def, "Name", None) or "") == pset_name
            ):
                return prop_def
    pset = model.create_entity(
        "IfcPropertySet",
        GlobalId=ifcopenshell.guid.new(),
        Name=pset_name,
        HasProperties=[],
    )
    model.create_entity(
        "IfcRelDefinesByProperties",
        GlobalId=ifcopenshell.guid.new(),
        RelatedObjects=[element],
        RelatingPropertyDefinition=pset,
    )
    return pset


def _set_single_property(model: Any, pset: Any, prop_name: str, raw_value: str) -> tuple[str, str]:
    """Set or add one ``IfcPropertySingleValue`` on ``pset``; return ``(old, new)`` strings."""
    old_val_str = ""
    existing_prop = None
    for prop in getattr(pset, "HasProperties", []) or []:
        if (getattr(prop, "Name", None) or "") == prop_name:
            existing_prop = prop
            nom = getattr(prop, "NominalValue", None)
            if nom is not None:
                old_val_str = _truncate_value(str(getattr(nom, "wrappedValue", nom)))
            break

    nominal = _parse_nominal_value(model, raw_value)
    new_val_str = _truncate_value(str(getattr(nominal, "wrappedValue", raw_value)))

    if existing_prop is not None and existing_prop.is_a("IfcPropertySingleValue"):
        existing_prop.NominalValue = nominal
    else:
        new_prop = model.create_entity(
            "IfcPropertySingleValue",
            Name=prop_name,
            NominalValue=nominal,
        )
        current_props = list(getattr(pset, "HasProperties", []) or [])
        current_props.append(new_prop)
        pset.HasProperties = current_props

    return old_val_str, new_val_str


def set_property(
    input_path: Path,
    output_path: Path,
    *,
    pset: str,
    prop: str,
    value: str,
    class_filter: str | None = None,
    global_ids: list[str] | None = None,
) -> tuple[dict[str, Any], bool]:
    """Write ``pset.prop = value`` on all selected elements into ``output_path``.

    Selectors combine by intersection when both are given. Returns
    ``(data, changes_capped)``; ``data`` holds output, elements_matched,
    properties_set and the changes diff (capped at ``MAX_CHANGES_REPORTED``).
    """
    cls = class_filter.strip() if class_filter and class_filter.strip() else None
    gids = [g for g in (global_ids or []) if g]

    probe_model = load_ifc(input_path)
    if cls:
        try:
            probe_model.by_type(cls)
        except Exception as exc:
            raise ToolError(
                "invalid_input",
                f"Unknown IFC class '{cls}'. Check spelling/casing, e.g. IfcWall, IfcSlab.",
                details={"reason": str(exc)[:200]},
            ) from exc
    candidates = list(probe_model.by_type(cls)) if cls else list(probe_model.by_type("IfcProduct"))

    gid_set = set(gids) if gids else None
    matched_guids = [
        str(getattr(el, "GlobalId", "") or "")
        for el in candidates
        if str(getattr(el, "GlobalId", "") or "")
        and (gid_set is None or str(getattr(el, "GlobalId", "")) in gid_set)
    ]
    if not matched_guids:
        raise ToolError(
            "element_not_found",
            "Selector matched 0 IFC elements.",
            hints=["Call ifc_query_elements to list valid GlobalIds and IFC classes."],
        )

    pset_name = pset.strip()
    prop_name = prop.strip()
    tmp_path = output_path.parent / (output_path.name + ".tmp")
    try:
        output_path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(input_path, tmp_path)
        work_model = ifcopenshell.open(str(tmp_path))
        matched_set = set(matched_guids)
        properties_set = 0
        changes: list[dict[str, str]] = []
        changes_capped = False
        for el in work_model.by_type("IfcObject"):
            egid = str(getattr(el, "GlobalId", "") or "")
            if egid not in matched_set:
                continue
            pset_obj = _get_or_create_pset(work_model, el, pset_name)
            old_str, new_str = _set_single_property(work_model, pset_obj, prop_name, value)
            properties_set += 1
            if len(changes) < MAX_CHANGES_REPORTED:
                changes.append(
                    {
                        "global_id": egid,
                        "pset": pset_name,
                        "prop": prop_name,
                        "old": old_str,
                        "new": new_str,
                    }
                )
            else:
                changes_capped = True
        work_model.write(str(output_path))
        ifcopenshell.open(str(output_path))
    except ToolError:
        raise
    except Exception as exc:
        with contextlib.suppress(OSError):
            output_path.unlink(missing_ok=True)
        raise ToolError(
            "internal_error",
            f"Failed to edit IFC properties: {exc}",
            retryable=True,
        ) from exc
    finally:
        tmp_path.unlink(missing_ok=True)

    data: dict[str, Any] = {
        "output": f"outputs/{output_path.name}",
        "elements_matched": len(matched_guids),
        "properties_set": properties_set,
        "changes": changes,
    }
    return data, changes_capped


__all__ = [
    "MAX_CHANGES_REPORTED",
    "MAX_GLOBAL_IDS",
    "MAX_PROP_VALUE_CHARS",
    "set_property",
    "validate_output_name",
]
