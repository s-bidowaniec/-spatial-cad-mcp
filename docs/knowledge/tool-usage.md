# Tool usage patterns

How to drive the MCP servers in this repo as an agent. Durable rules: [AGENTS.md](../../AGENTS.md); design rationale: [docs/tool-design.md](../tool-design.md); the wire contract: [schemas/envelope.v1.schema.json](../../schemas/envelope.v1.schema.json).

## The call order that works

1. `*_summary` first (`ifc_summary`, `dxf_summary`, `gis_summary`, `hdm_summary`, `mesh_summary`) - counts and extents, never raw geometry.
2. Then the paginated query tools (`ifc_query_elements`, `dxf_query_entities`, `gis_query_features`, `hdm_query_timeseries`, `mesh_query_quality`, ...). `limit` hard cap 200; follow `meta.pagination.next_offset` / `has_more` instead of dumping everything.
3. Then detail tools (`ifc_get_psets`, `dxf_extract_text`, `gis_query_point`, `hdm_query_profiles`, `mesh_sample_elevation`, `mesh_section_line`, `mesh_diff`, ...).
4. Mutators only when the task asks for an output: they write NEW files under `<workspace>/outputs/` and return a structured diff. Inputs are never modified; name collisions fail with `output_exists`.

## Error codes you will actually hit

| code | meaning | what to do |
| --- | --- | --- |
| `workspace_not_configured` | `MCP_CAD_WORKSPACE_ROOT` unset or not a directory | set it to an existing directory and restart the server |
| `path_traversal` | path contains `..` | use a path relative to the workspace root |
| `path_outside_workspace` | absolute path or symlink escaping the root | same - and do not try to work around the sandbox |
| `file_not_found` | no such file inside the root | check the path with the group's summary tool first |
| `invalid_input` | bad argument (e.g. `limit < 1`) | the message says what is wrong and usually gives an example value |
| `cannot_open_ifc` / format equivalents | unparseable or unsupported format | run the summary tool on the file first to see what it is |
| `output_exists` | mutator output name already taken | pick a new `output_*_name` under `outputs/` |
| `internal_error` | unexpected bug, `retryable: true` | retry once, then report with the server's stderr logs |

## Anti-patterns

- Never ask for raw geometry or full entity trees - paginate instead.
- Never read a model file directly into your context - files can be hundreds of MB; use summary/query tools.
- Never try `..`, absolute paths or symlink tricks - the sandbox rejects them by design.
- Never call a mutator without an `output_*_name`; mutators never overwrite.

## Lessons log

Append dated entries when a task teaches you something reusable:

- 2026-11-25 - The envelope contract is pinned by `schemas/envelope.v1.schema.json`; Python (`mcp_cad_core`) and Rust (`cad-envelope`) must stay in lockstep - `tests/test_envelope_schema.py` enforces it. `tools/check_tools.py` lints tool names/descriptions/annotations.
