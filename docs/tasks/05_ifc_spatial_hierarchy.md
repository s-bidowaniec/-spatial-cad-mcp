# Task 05 - `ifc_query_spatial`: IFC spatial hierarchy browser (mcp-ifc, read-only)

## Goal
Roadmap item: agents can query elements but cannot browse the spatial structure
(IfcSite > IfcBuilding > IfcBuildingStorey > containers) or containment relations.

## Tool spec

- Name: `ifc_query_spatial`. Kind: **read-only**.
- Params: `ifc_path`, `root_global_id: Option<String>` (default: project/site root),
  `depth: Option<usize>` (max tree depth, default 3, cap 10), `limit` (cap 200), `offset`.
- Returns `data`: flattened tree rows `{ "global_id", "class", "name", "level": int,
  "parent_global_id": Option<String>, "contained_elements": N }` (containment count via
  IfcRelContainedInSpatialStructure), paginated, ordered by (level, class, name) - deterministic.
- Errors: `file_not_found`, `cannot_open_ifc`, `element_not_found`, `invalid_input`,
  sandbox codes - same vocabulary as existing tools.

## Implementation notes
- `packages/mcp-ifc/src/mcp_ifc/tools.py`: plain `@safe_tool` function; use the model cache
  (`model_cache.py`) like the other tools; resolve paths via `workspace_sandbox()`.
- Walk `IfcRelDecomposes` (aggregation) from the root; count contained elements via
  `IfcRelContainedInSpatialStructure`. Do NOT expand element lists here - that is
  `ifc_query_elements` with a new optional `container_global_id` filter (add it in the
  same task: filter elements whose `ContainedInStructure` rel points at the container).
- `server.py`: mirror wrapper with `@mcp.tool()` and identical name.

## Tests (`tests/test_ifc_tools.py` style, synthetic IFC built in tmp_path with ifcopenshell)
- Build a tiny synthetic model: IfcProject > IfcSite > IfcBuilding > 2 storeys, one with
  3 walls. Assert tree rows, levels, containment counts, pagination clamp at 200.
- `element_not_found` for unknown root_global_id; `invalid_input` for depth 0 / > 10.

## Docs
Root `AGENTS.md` index, `packages/mcp-ifc/README.md`.

## Acceptance
`uv run pytest` + `ruff check .` + `ruff format --check .` green; response stays small
(counts + paginated rows, never full element dumps).

## Out of scope
Geometry of spatial elements, non-spatial relations (IfcRelConnects etc.) - follow-up task.
