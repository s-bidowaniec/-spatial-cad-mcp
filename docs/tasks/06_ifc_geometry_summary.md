# Task 06 - IFC geometry summaries: bbox per element and per class (mcp-ifc, read-only)

## Goal
Roadmap item: geometry-level summaries via `ifcopenshell.geom`, but WITHOUT ever returning
raw geometry to the agent context - bounding boxes and counts only.

## Tool specs

### `ifc_element_bbox` (read-only)
- Params: `ifc_path`, `global_id`, `include_children: bool` (default false - for
  spatial elements, union of contained elements' bboxes; cap children at 200, clamped).
- Returns `data`: `{ "global_id", "class", "bbox": { "min": [x,y,z], "max": [x,y,z] },
  "center": [x,y,z], "geometry_tris": N }` (triangle count from the shape, not vertices).
- If `ifcopenshell.geom` (OCCT) is unavailable in the environment: envelope error
  `not_implemented` with message "geometry kernel unavailable; install ifcopenshell[occ]"
  - do not crash.

### `ifc_geometry_summary` (read-only)
- Params: `ifc_path`, `class_filter: Option<String>` (e.g. `IfcWall`).
- Returns per-class aggregate: `{ "classes": [ { "class", "count", "bbox_union": {...},
  "bbox_min_area_element": { "global_id", "name" } } ] }`, paginated (cap 200 classes).

## Implementation notes
- `tools.py`: guarded import `ifcopenshell.geom` inside the function; settings
  `USE_WORLD_COORDS = True`, output `TRIANGULATED` mesh; compute bbox from vertices.
- Cache shapes per (file, global_id) in `model_cache.py` within a TTL - large models.
- Never return vertex arrays; only bbox/center/tri counts.

## Tests
- Synthetic IFC with one IfcWall with a simple extruded profile (built via ifcopenshell in
  tmp_path): assert bbox min/max match the profile dims, `geometry_tris` > 0.
- Missing-kernel path: monkeypatch the geom import to fail -> `not_implemented` envelope.

## Docs
Root `AGENTS.md` index, `packages/mcp-ifc/README.md`, roadmap line in `docs/architecture.md`.

## Acceptance
`uv run pytest`, ruff green; context budget respected (assert response size small in test).

## Out of scope
Full mesh export, section cuts, glTF export (mcp-mesh/glTF roadmap covers export).
