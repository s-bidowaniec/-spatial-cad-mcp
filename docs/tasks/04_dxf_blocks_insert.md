# Task 04 - BLOCK definitions & INSERT expansion (mcp-dxf, read-only)

## Goal
Roadmap item: agents cannot see what is inside BLOCKs or how INSERTs place them. Add a
paginated block query and optional INSERT expansion in entity queries.

## Tool specs

### `dxf_query_blocks` (read-only)
- Params: `input_dxf`, `name_filter: Option<String>` (regex, like `dxf_query_entities`),
  `limit` (cap 200), `offset`.
- Returns `data.blocks[]`: `{ "name": "...", "base_point": [x,y], "entity_counts": {
  "LINE": 3, ... }, "insert_count": N (how many INSERTs reference it) }`, plus pagination.
- Envelope errors: `file_not_found`, `cannot_open_dxf`, `invalid_input`, sandbox codes.

### `dxf_query_entities` extension
- New optional param `expand_inserts: bool` (default false). When true, each INSERT entity
  is returned with `"expanded": [ {entity fields with transform applied}, ... ]` - the
  block's entities transformed by insert point + scale (x,y) + rotation (degrees).
  Expansion counts against the page limit; expanded sub-entities are capped (e.g. 50 per
  INSERT, noted in meta) to protect context.

## Implementation notes
- `parser.rs`: parse BLOCKS section into `BlockDef { name, base_point, entities: Vec<EntityRecord> }`;
  keep raw group codes per entity. Build `insert_refs: HashMap<block_name, usize>` while parsing ENTITIES.
- Transform helper: `apply_insert_transform(point, insert)` = rotate(scale(p - base)) + insert_point.
  Add unit tests for rotation/scale math (0/90/180 deg, negative scale = mirror).
- `server.rs`: new tool + param; reuse the existing pagination/truncation helpers from cad-envelope.

## Tests
- Synthetic DXF with one BLOCK (2 LINE + 1 CIRCLE) and two INSERTs (one rotated 90 deg, one scaled 2x):
  `dxf_query_blocks` counts; `dxf_query_entities` with `expand_inserts: true` returns transformed coords
  (assert exact transformed coordinate of a known point).
- Anonymous blocks (`*D...`, `*U...`) listed but expansion best-effort; document behavior.
- Pagination: limit/offset over blocks; clamp at 200.

## Docs
Root `AGENTS.md` index, `packages/mcp-dxf/README.md`, roadmap line in `docs/architecture.md`.

## Acceptance
`cargo test -p mcp-dxf` + clippy green; no raw block dumps - counts + paginated records only.

## Out of scope
ATTRIB/ATTDEF handling inside INSERTs (note as follow-up), XREF external references, dynamic blocks.
