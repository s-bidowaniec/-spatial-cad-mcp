# Task 01 - `dxf_edit_entities`: generic DXF edit mutator (mcp-dxf)

## Goal
Close the "DXF edit" gap: today the only mutators are specialized transforms. Add one
generic, safe mutator that copies an ASCII DXF and applies a batch of edits, writing a
NEW file under `<workspace>/outputs/`.

## Tool spec

- Name: `dxf_edit_entities`. Kind: **mutating**.
- Params (`DxfEditParams`):
  - `input_dxf: String` - relative to workspace root (sandbox-resolved).
  - `output_dxf_name: String` - must end with `.dxf`, no path separators; written under
    `<workspace>/outputs/`; collision -> `output_exists`.
  - `select: DxfSelect` - all fields optional, ANDed:
    `layer: Option<String>`, `entity_type: Option<String>` (e.g. `LINE`, `LWPOLYLINE`),
    `handle_from: Option<String>`, `handle_to: Option<String>` (hex handle range, inclusive),
    `bbox: Option<[f64;4]>` (2D [min_x, min_y, max_x, max_y]; entity bbox intersects).
  - `ops: Vec<DxfEditOp>` - applied in order to every selected entity:
    - `{ "op": "set_layer", "layer": "..." }`
    - `{ "op": "set_color", "color": 1..255 }` (ACI)
    - `{ "op": "set_linetype", "linetype": "..." }`
    - `{ "op": "set_lineweight", "lineweight": 0..211 }`
    - `{ "op": "move", "dx": f64, "dy": f64 }`
    - `{ "op": "rotate", "cx": f64, "cy": f64, "angle_deg": f64 }`
    - `{ "op": "scale", "cx": f64, "cy": f64, "factor": f64 }`
    - `{ "op": "delete" }`
  - `limit: Option<usize>` - max entities to edit (cap 200, clamped with `meta.limit_clamped_to`).
- Returns envelope `data`: `{ "output": "outputs/<name>", "selected": N, "edited": N,
  "deleted": N, "per_op": { "move": 12, ... }, "entity_type_counts_before": {...},
  "entity_type_counts_after": {...} }`.
- Error codes: `file_not_found`, `cannot_open_dxf`, `invalid_input` (empty ops,
  unknown op, bbox min>max), `output_exists`, `path_traversal`/`path_outside_workspace`,
  `internal_error`.

## Implementation notes
- Parser: `packages/mcp-dxf/src/parser.rs` (`parse_dxf_file`, `EntityRecord`, `export_dxf_slice`
  shows the verbatim group-code copy pattern). Extend `DxfDocument`/`EntityRecord` with
  handle + layer + color/linetype/lineweight accessors if missing; keep the raw group-code
  vector per entity so edits rewrite only the affected groups (lossless for untouched entities).
- Writer: build on the verbatim writer used by `export_dxf_slice` (HEADER/TABLES/ENTITIES/ENDSEC/EOF).
  Add a small `dxf_writer` module reused later by task 02.
- Server: `packages/mcp-dxf/src/server.rs` - new `#[tool]` method in the `tool_router` impl,
  params struct with per-field doc comments (they become the JSON schema).

## Tests (cargo, in server.rs / parser.rs)
- happy path: select by layer, move + set_color, verify output file parses and counts in diff match.
- delete op reduces counts; untouched entities byte-identical (compare group streams).
- rejection paths: `..` traversal, missing input, existing output name, empty ops, bad bbox.
- determinism: two runs produce identical output bytes.

## Docs to update
Root `AGENTS.md` tool index, `packages/mcp-dxf/README.md`.

## Acceptance
`cargo test -p mcp-dxf` green, `cargo clippy -p mcp-dxf` clean, envelope examples above
hold, inputs never modified (test asserts input file hash unchanged).

## Out of scope
Binary DXF (task 03), BLOCK/INSERT expansion (task 04), 3D transforms (Z move only if trivial).
