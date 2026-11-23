# Task 08 - `ifc_set_property`: edit pset properties on a copy (mcp-ifc, mutating)

## Goal
Second IFC mutator: change property values on selected elements, writing a NEW IFC under
`outputs/`. This is the "IFC edit/write" capability agents need for QA fixes and tagging.

## Tool spec

- Name: `ifc_set_property`. Kind: **mutating**.
- Params: `ifc_path`, `output_name` (`.ifc` under `outputs/`, collision -> `output_exists`),
  selector: `class_filter: Option<String>` and/or `global_ids: Vec<String>` (cap 200;
  at least one required), then `pset: String`, `prop: String`, `value: String`.
- Behavior: copy input to outputs; for each selected element, find-or-create the
  property set (`IfcPropertySet` attached via `IfcRelDefinesByProperties`) and set
  `IfcPropertySingleValue` (NominalValue as IfcText/IfcLabel, or int/float when parseable);
  write file; re-open to validate.
- Returns `data`: `{ "output": "outputs/<name>", "elements_matched": N,
  "properties_set": N, "changes": [ { "global_id", "pset", "prop", "old": "...", "new": "..." } ] }`
  (changes list capped at 200 with `meta` note; long values truncated with marker).
- Errors: same vocabulary as task 07 + `element_not_found` when selector matches nothing
  (ok:false, retryable:false, hints to run ifc_query_elements first).

## Implementation notes
- Use ifcopenshell API (`ifcopenshell.api.property.edit_pset` if available in the pinned
  version - check `packages/mcp-ifc/pyproject.toml`; otherwise manipulate entities
  directly). Keep to the pinned ifcopenshell version; no new heavy deps.
- Reuse the outputs-dir sandbox pattern from task 07 - factor a shared helper
  `_outputs_path(sandbox, name)` in `tools.py` or `mcp_cad_core.sandbox` if it fits.
- `server.py` wrapper.

## Tests
- Synthetic IFC with walls having a pset: set existing prop (old value captured in
  changes), create new pset+prop on element without it, numeric value stored as
  IfcReal/IfcInteger when parseable else IfcText; output re-opens; input unchanged.
- Rejections: no selector, selector matching nothing, bad output name, existing output.

## Docs
Root `AGENTS.md` index, `packages/mcp-ifc/README.md`.

## Acceptance
`uv run pytest`, ruff green; changes diff matches envelope spec; input file hash unchanged.

## Out of scope
Deleting properties/elements, quantity sets (IfcElementQuantity) editing - follow-ups.
