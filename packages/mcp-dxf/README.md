# mcp-dxf

Pure-Rust stdio MCP server for ASCII DXF drawings (rmcp + cad-envelope + cad-sandbox).
Refuses to start without `MCP_CAD_WORKSPACE_ROOT`; every tool answers with the standard
envelope (`ok`/`data`/`meta` or `ok:false` + `error{code,message,retryable,hints}`).

## Supported input formats

| Format | Detection | Support | Tool behavior |
| --- | --- | --- | --- |
| ASCII DXF (R10-R2018+) | section heuristics (`SECTION` + `ENTITIES`/`HEADER`/`EOF`) | full read + edit | `data.format = "dxf_ascii"`, `binary: false` in `dxf_summary` |
| Binary DXF (R12+) | magic `AutoCAD Binary DXF\r\n\x1a\0` | not parsed (MVP) | `error.code = "unsupported_format"` + conversion hints |
| DWG (R10-R2018+) | magic `AC10xx` at offset 0 | never parsed | `error.code = "unsupported_format"` with version label + hints |
| unknown | none of the above | - | parsed as ASCII; fails with `cannot_open_dxf` if malformed |

## Tools

Read-only: `dxf_summary`, `dxf_query_layers`, `dxf_query_entities`, `dxf_extract_text`.
Mutating (write a NEW file under `<workspace>/outputs/`, never modify inputs, refuse
`output_exists`): `dxf_export_slice`, `dxf_to_mesh`, `dxf_centerline`, `dxf_labels_to_3d`.
