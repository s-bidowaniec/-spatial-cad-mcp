# Task 03 - DXF format detection: binary DXF + DWG, structured errors (mcp-dxf)

## Goal
Today `parse_dxf_bytes` hard-fails on `AutoCAD Binary DXF` and does not recognize DWG at
all - an agent gets a generic error with no path forward. Detect formats explicitly and
return a structured `unsupported_format` envelope with conversion hints; optionally parse
R12 binary ENTITIES.

## Tool spec changes

- `dxf_summary` gains fields in `data`: `format: "dxf_ascii" | "dxf_binary" | "dwg" |
  "unknown"`, `binary: bool`, `acad_version`, `acad_release` (already present for ASCII).
- For binary/DWG input, read-only tools return envelope error:
  `{ "ok": false, "error": { "code": "unsupported_format", "message": "Binary DXF (R12)
  is not parsed yet; convert to ASCII DXF (e.g. LibreDWG `dwg2dxf`) and retry",
  "retryable": false, "hints": ["convert with LibreDWG/ODA File Converter", "then call
  dxf_summary again"] } }`.
- DWG detection: magic `AC10xx` at offset 0 (e.g. `AC1015` = R2000 DWG). Message names the
  DWG version label (reuse `acad_version_label`) and suggests conversion; never attempt
  to parse proprietary DWG content.

## Implementation notes
- `parser.rs`: add `pub fn detect_format(raw: &[u8]) -> DxfFormat` (checks `AutoCAD Binary
  DXF\r\n\x1a\0` header, `AC10` DWG magic, else ASCII heuristics: contains `SECTION`/`ENTITIES`).
  Wire into `parse_dxf_bytes`/`parse_dxf_file` before the ASCII path; keep the existing
  error text for ASCII parse failures (`cannot_open_dxf`).
- Stretch (only if time permits, clearly separated commit): minimal R12 binary DXF reader
  for the ENTITIES section only (group-code pairs: int16 code + value by code class), feeding
  the same `EntityRecord` pipeline. If not done, the structured error above is enough for MVP.
- `server.rs`: map `DxfFormat::Binary/Dwg` in every tool to `unsupported_format`.

## Tests
- Synthetic fixtures built in tests (no real files): ASCII DXF string, fake binary header
  bytes, fake DWG magic `AC1015` + junk; assert `detect_format` and the envelope codes.
- `dxf_summary` on ASCII fixture reports `format: "dxf_ascii"`, `binary: false`.

## Docs
Root `AGENTS.md` index note, `packages/mcp-dxf/README.md` (supported formats table),
`docs/architecture.md` roadmap line update.

## Acceptance
`cargo test -p mcp-dxf` green; agents get actionable `unsupported_format` for binary/DWG
instead of a raw parse error.

## Out of scope
Full binary DXF writer, any DWG parsing, ODA/LibreDWG integration as a dependency
(stay pure Rust; shelling out is NOT allowed inside the sandbox server).
