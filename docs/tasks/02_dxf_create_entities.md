# Task 02 - general DXF writer + `dxf_create` / `dxf_add_entities` (mcp-dxf)

## Goal
Close the "DXF write from scratch / add entities" gap: today only 3D polylines can be
written (`write_dxf_polylines_3d` in `cad3d.rs`). Add a general ASCII DXF writer and two
mutators: create a new drawing, or copy an existing one and append entities.

## Tool specs

### `dxf_create` (mutating)
- Params: `output_dxf_name: String` (`.dxf`, under `outputs/`, no separators),
  `entities: Vec<DxfNewEntity>`, `acad_version: Option<String>` (default `AC1015` / R2000),
  `layers: Vec<String>` (declared in TABLES, default `["0"]`).
- `DxfNewEntity` tagged union, all coordinates absolute XY(Z):
  - `LINE { layer, start:[x,y,z?], end:[x,y,z?], color? }`
  - `CIRCLE { layer, center:[x,y,z?], radius, color? }`
  - `ARC { layer, center, radius, start_angle_deg, end_angle_deg, color? }`
  - `LWPOLYLINE { layer, points:[[x,y],...], closed: bool, color? }`
  - `POLYLINE3D { layer, points:[[x,y,z],...] }`
  - `TEXT { layer, insert:[x,y,z?], height, text, rotation_deg?, color? }`
  - `POINT { layer, at:[x,y,z?] }`
- Returns `data`: `{ "output": "outputs/<name>", "created": N, "layers": [...],
  "bbox": [min_x,min_y,min_z,max_x,max_y,max_z] }`.
- Caps: max 200 entities per call (clamp + `meta.limit_clamped_to`); text truncated at
  256 chars with marker; `invalid_input` on bad numbers (NaN/inf rejected).

### `dxf_add_entities` (mutating)
- Params: `input_dxf`, `output_dxf_name`, `entities` (same union), optional `select`+`ops`
  from task 01 applied first (reuse the same edit pipeline).
- Returns structured diff: counts before/after, added per type, output path.

## Implementation notes
- New module `packages/mcp-dxf/src/writer.rs`: `fn write_dxf(path, header_vars, layers, entities, blocks)`
  emitting a minimal valid R2000 ASCII DXF (HEADER with `$ACADVER`, TABLES with LAYER
  records, ENTITIES, EOF). Reuse `strip_autocad_formatting`-style normalization.
  Refactor `write_dxf_polylines_3d` to use it (keep behavior identical - existing tests must pass).
- Entity serialization: group codes per type (LINE: 10/20/30, 11/21/31; CIRCLE: 10/20/30, 40;
  ARC: +50/51; LWPOLYLINE: 90 count, 70 flags, 10/20 pairs; TEXT: 10/20/30, 40 height, 1 text, 50 rot).
- Server wiring in `server.rs` as in task 01.

## Tests
- Round-trip: create file with one of each entity type, re-parse with `parse_dxf_file`,
  assert counts and key coordinates.
- `dxf_add_entities` on a synthetic input: input hash unchanged, output contains old+new counts.
- Rejections: NaN coords, empty entities list, output collision, traversal, >200 clamp.

## Docs
Root `AGENTS.md` index, `packages/mcp-dxf/README.md`.

## Acceptance
`cargo test -p mcp-dxf` + clippy green; output files open in a reference DXF reader
(validate with the repo parser round-trip; note in PR how external validation was done).

## Out of scope
HATCH/SPLINE/BLOCK definitions (task 04 covers blocks read), binary output, DWG output.
