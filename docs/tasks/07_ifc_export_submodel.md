# Task 07 - `ifc_export_submodel`: first IFC mutator (mcp-ifc)

## Goal
Roadmap item: the first mutating IFC tool. Export a subset of an IFC model to a NEW file
under `<workspace>/outputs/` - inputs are never modified.

## Tool spec

- Name: `ifc_export_submodel`. Kind: **mutating**.
- Params: `ifc_path`, `output_name` (must end `.ifc`, no separators, under `outputs/`,
  collision -> `output_exists`), and at least one selector (else `invalid_input`):
  - `class_filter: Vec<String>` (e.g. `["IfcWall", "IfcSlab"]`), and/or
  - `global_ids: Vec<String>` (cap 200), and/or
  - `container_global_id: String` (export everything contained in a spatial container,
    uses task 05 containment).
- Behavior: load model (cache), collect matching product elements, write a new IFC file
  with ifcopenshell containing those elements (keep required context: spatial container
  chain, type objects, materials - simplest robust strategy: copy whole file then remove
  non-matching products, preserving header; document the chosen strategy in the PR).
- Returns `data`: `{ "output": "outputs/<name>", "elements_before": N, "elements_after": M,
  "by_class": { "IfcWall": 12, ... }, "file_size_bytes": S }`.
- Errors: `file_not_found`, `cannot_open_ifc`, `invalid_input` (no selector / bad name),
  `output_exists`, unknown global_ids/container -> still export the rest but report
  `missing_global_ids: [...]`, sandbox codes, `internal_error` retryable.

## Implementation notes
- `tools.py`: `@safe_tool` function; write via `ifcopenshell` copy + remove non-matching
  products, or `ifcopenshell.api` copy - pick the approach that keeps the file valid;
  validate the output by re-opening it before returning ok.
- Sandbox: resolve input via `workspace_sandbox()`; build `<root>/outputs/<name>` through
  the sandbox with an `outputs/` prefix allowlist (check `shared/mcp_cad_core/sandbox.py`
  for the existing pattern).
- `server.py`: `@mcp.tool()` wrapper with the same name.

## Tests
- Synthetic IFC (tmp_path) with walls + slabs in two storeys: export by class, by
  global_ids, by container; re-open output and assert counts; input file hash unchanged.
- Rejections: no selector, `.txt` output name, existing output, traversal in input.

## Docs
Root `AGENTS.md` index + rule 3 example list (add `ifc_export_submodel`),
`packages/mcp-ifc/README.md`.

## Acceptance
`uv run pytest`, ruff green; output IFC re-opens cleanly; envelope matches spec.

## Out of scope
Property editing (task 08), geometry healing, IFC schema conversion (IFC2x3->IFC4).
