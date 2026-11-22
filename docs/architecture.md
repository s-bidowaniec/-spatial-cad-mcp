# Architecture

## Goals

Deterministic, context-safe MCP tooling for CAD/BIM file formats, optimized for the caller
being **another LLM agent** rather than a human at a GUI. Consequences:

- Responses are compact JSON envelopes: counts, summaries, bounded lists - never raw geometry
  or full entity trees (an IFC file can be hundreds of MB; the agent's context cannot).
- Errors are structured (`ok: false` + machine-readable `code`), so an agent can branch and
  retry without parsing prose or stack traces.
- Behavior is deterministic: same input, same envelope shape; pagination is explicit
  (`limit`/`offset`, hard cap 200 per page).

## Language split

| Area | Language | Why |
| --- | --- | --- |
| IFC inspection/validation (mvp) | Python + ifcopenshell | ifcopenshell is the reference IFC library; no comparable Rust ecosystem exists yet |
| Shared core (sandbox/envelopes/pagination/logging) | Python | ships with all Python servers |
| DXF server (scaffold) | Rust + rmcp | single static binary per server; `dxf` crate for parsing (roadmap) |
| GIS server (scaffold) | Rust + rmcp | GeoPackage via `rusqlite`, GeoJSON natively; single binary (roadmap) |
| Hydrodynamic server (scaffold) | Rust + rmcp | HEC-RAS HDF via `hdf5` crate, netCDF via `netcdf` crate, TELEMAC Selafin natively (roadmap) |
| Mesh server (scaffold) | Rust + rmcp | STL/OBJ/glTF parsing is simple and fast in Rust (roadmap) |
| Sandbox (Rust) | Rust (`cad-sandbox`) | identical rules to the Python sandbox, shared by all Rust binaries |
| CLI (`cadctl`) | Rust | streaming IFC header/counts inspection with zero CAD dependencies |

Both language ecosystems speak the same envelope vocabulary and the same sandbox rules; see
`docs/security.md` for the threat model.

## Transport and process model

- Default transport: **stdio**. One server process per tool domain, launched by the MCP host
  from `examples/mcp.json`.
- stdout is owned by the MCP protocol. All logging goes to stderr
  (`mcp_cad_core.log` / `eprintln!`-based logger).
- Each Python server process keeps a small LRU cache of parsed models (keyed by path +
  mtime/size) so agent tool-call sequences don't re-parse the same file. Cache correctness is
  stat-based: an unchanged (mtime,size) pair reuses the model, otherwise the file is reopened.

## Request flow (every tool)

1. Validate input arguments (types via the MCP SDK schemas; semantics inside the tool).
2. Resolve the file path through the workspace sandbox (`MCP_CAD_WORKSPACE_ROOT`).
3. Load/parse the document (cached for IFC).
4. Compute a **bounded** result (page size caps, truncation of long values).
5. Return the standard envelope (`ok`/`data`/`meta` or `ok:false`/`error`).

Failures at step 2 produce the error codes `workspace_not_configured`, `path_traversal`,
`path_outside_workspace`, `file_not_found`; parse failures produce `cannot_open_ifc` (or
format-specific equivalents); argument failures produce `invalid_input`; anything unexpected is
`internal_error` (with `retryable: true` for the agent's benefit). Real exceptions never leak:
the `safe_tool` decorator converts them into envelopes and logs details to stderr.

## Roadmap

- mcp-ifc: geometry-level summaries (bounding boxes via `ifcopenshell.geom`), spatial-hierarchy
  browser, first mutating tool (`ifc_export_submodel`, writing to `outputs/`).
- mcp-dxf: `dxf_summary`, `dxf_query_entities`, `dxf_extract_text` (Rust `dxf` crate).
- mcp-gis: `gis_summary`, `gis_query_features`, `gis_extract_schema` (GeoPackage via `rusqlite`,
  GeoJSON natively).
- mcp-hdm: `hdm_summary`, `hdm_query_timeseries`, `hdm_query_profiles` (HEC-RAS HDF, SWMM,
  TELEMAC Selafin, Delft3D netCDF).
- mcp-mesh: `mesh_summary` (OBJ/STL/glTF), `mesh_query_parts`, `mesh_slice_bbox`.
- Shared JSON Schema for the envelope in both ecosystems for client-side validation.
