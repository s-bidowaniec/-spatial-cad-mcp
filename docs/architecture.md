# Architecture

## Status

**Pre-alpha, MVP across all five tool groups.** `mcp-ifc` (Python) and all four Rust MCP servers
(`mcp-dxf`, `mcp-gis`, `mcp-hdm`, `mcp-mesh`) implement sandboxed, envelope-shaped read-only and
output-isolated mutating tools. Everything here may still evolve before the first tagged release.

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
| DXF server (mvp) | Rust + rmcp | pure-Rust streaming ASCII DXF parser with LibreDWG crease repair, Z=0 audit, layer/entity/text queries, lossless slice export, `3DFACE->mesh`, 3D centreline, and elevation-labels to 3D |
| GIS server (mvp) | Rust + rmcp | GeoPackage via `rusqlite` + WKB decoder, GeoJSON natively, CRS sanity checks, point-in-polygon/radius queries, bbox GeoJSON export |
| Hydrodynamic server (mvp) | Rust + rmcp | pure-Rust TELEMAC Selafin (`.slf`/`.res`), EPA SWMM (`.inp`), and SMS `.2dm` with time series, profiles, and CSV export; HEC-RAS HDF / Delft3D netCDF on roadmap |
| Mesh server (mvp) | Rust + rmcp | pure-Rust `.2dm`/`.obj`/`.stl` loader with edge-incidence topology, watertightness, element QA, elevation/section sampling, mesh diff, greedy diagonal-flip grading, plane cutting, exact cut/fill volumes & zero-lines, DXF export, and bbox slicing |
| Envelopes & Sandbox (Rust) | Rust (`cad-envelope`, `cad-sandbox`) | identical envelope/pagination/sandbox rules to `mcp_cad_core`, shared by all Rust binaries |
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
  browser. Done: first mutating tool (`ifc_export_submodel`, writing a validated new IFC to
  `outputs/`).
- mcp-dxf: 2D-to-3D polyline draping onto mesh surfaces (`dxf_drape_mesh`), block/INSERT expansion.
- mcp-gis: Shapefile / FlatGeobuf / GeoTIFF raster summary & clip support.
- mcp-hdm: HEC-RAS HDF and Delft3D netCDF readers.
- mcp-mesh: glTF/GLB support, spatial-index acceleration for multi-million-face meshes, constrained Delaunay triangulation.
- `cadctl`: multi-format quick inspect across DXF, mesh, GIS, and HDM files.
- Shared JSON Schema for the envelope in both ecosystems for client-side validation.
