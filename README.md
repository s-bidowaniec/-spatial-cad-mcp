# spatial-cad-mcp

[![CI](https://github.com/s-bidowaniec/spatial-cad-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/s-bidowaniec/spatial-cad-mcp/actions/workflows/ci.yml)
[![status: pre-alpha / concept](https://img.shields.io/badge/status-pre--alpha%20%2F%20concept-orange)](#project-status)

## Project status: pre-alpha, concept stage

This repository is a **pre-alpha monorepo**. All five MCP servers (`mcp-ifc` in Python; `mcp-dxf`,
`mcp-gis`, `mcp-hdm`, and `mcp-mesh` in Rust) now implement sandboxed MVP tools speaking the same
JSON envelope contract. Tool names, response envelopes and the package layout may still evolve
before the first tagged release.

Modular, deterministic **MCP servers** for CAD / BIM / GIS / hydrodynamic-model / mesh workflows,
designed for **AI-to-AI** use: compact summaries, paginated queries, strict workspace sandboxing
and structured error envelopes instead of raw geometry dumps.

Four basic tool groups:

| Group | Package | Scope | Status |
| --- | --- | --- | --- |
| BIM | `mcp-ifc` | IFC models: summary, element queries, psets, validation | MVP (Python) |
| CAD | `mcp-dxf` | 2D/3D ASCII DXF: summary, LibreDWG repair, Z=0 audit, queries, lossless slice, `3DFACE->mesh`, 3D centreline, height-labels to 3D | MVP (Rust) |
| GIS | `mcp-gis` | GeoPackage (`.gpkg`) & GeoJSON: summary, CRS audit, schema, feature/point queries, bbox export | MVP (Rust) |
| HDM | `mcp-hdm` | Open-standard HDM (TELEMAC Selafin `.slf`/`.res`, EPA SWMM `.inp`, SMS `.2dm`): summary, time series, profiles, CSV export | MVP (Rust) |

plus 2D/3D mesh & CAD surface workflows (`mcp-mesh`, MVP in Rust: `.2dm`/`.obj`/`.stl` topology, QA, elevation/sections, mesh diff, diagonal-flip grading, cut-plane contours, exact cut/fill volumes & zero-lines, DXF export, bbox slice) and the `cadctl` CLI.

Monorepo layout (Python where the ecosystem is strongest, Rust everywhere else):

```
spatial-cad-mcp/
├── packages/
│   ├── mcp-ifc/          # MVP server: IFC inspection/query/validation (Python, ifcopenshell)
│   ├── mcp-dxf/          # MVP server: ASCII DXF summary, layer/entity/text queries & slice export (pure Rust, rmcp)
│   ├── mcp-gis/          # MVP server: GeoPackage & GeoJSON summary, schema, queries, bbox export (Rust, rusqlite, rmcp)
│   ├── mcp-hdm/          # MVP server: TELEMAC Selafin, SWMM .inp, SMS .2dm summary, time series, profiles & CSV export (pure Rust, rmcp)
│   └── mcp-mesh/         # MVP server: 2D/3D mesh (.2dm/.obj/.stl) topology, QA, elevation/sections & bbox slice (pure Rust, rmcp)
├── shared/
│   └── mcp_cad_core/     # sandboxing, response envelopes, pagination, stderr logging (Python)
├── crates/
│   ├── cad-envelope/     # Rust twin of response envelopes, pagination & truncation
│   ├── cad-sandbox/      # Rust twin of the Python sandbox (cad-sandbox)
│   └── cadctl/           # fast CLI: IFC header/summary inspection + sandbox checks
├── examples/             # MCP client config (mcp.json) + sample agent prompts
├── docs/                 # architecture, tool design guidelines, security model
└── tests/                # pytest suite for sandboxing and tool error handling
```

## MCP tools (mcp-ifc, MVP)

| Tool | Kind | What it returns |
| --- | --- | --- |
| `ifc_summary` | read-only | Schema (IFC2X3/IFC4/...), length unit, total counts, per-class element counts |
| `ifc_query_elements` | read-only | Paginated elements (GlobalId, class, name, type, container) filtered by class / name regex |
| `ifc_get_psets` | read-only | Property sets of one element by GlobalId (truncated values, counts) |
| `ifc_validate_required_props` | read-only | QA audit: which elements of a class miss required properties |

Every tool returns an envelope:

```json
{ "ok": true,  "data": { "...": "..." }, "meta": { "pagination": { "total": 312, "next_offset": 50 } } }
{ "ok": false, "error": { "code": "path_outside_workspace", "message": "Path resolves outside the workspace root", "retryable": false } }
```

## Install (uv workspace, Python 3.11+)

```bash
uv sync --all-packages            # install all Python workspace members
uv run pytest                     # sandbox + tool error handling suite
uv run ruff check . && uv run ruff format --check .
```

## Run the servers (stdio)

The one required environment variable is `MCP_CAD_WORKSPACE_ROOT`; servers refuse to start
without it, and no tool can touch anything outside it (see `docs/security.md`).

```bash
MCP_CAD_WORKSPACE_ROOT=/abs/path/to/workspace uv run --project packages/mcp-ifc mcp-ifc
```

MCP client config: see [`examples/mcp.json`](examples/mcp.json).

## Rust side

```bash
cargo check --workspace   # build all Rust crates
cargo test --workspace    # cad-sandbox unit tests etc.
cargo run -p cadctl -- inspect path/to/model.ifc --top 10
cargo run -p mcp-dxf      # ASCII DXF MCP server over stdio (rmcp 3.x)
cargo run -p mcp-gis      # GeoPackage/GeoJSON MCP server over stdio (rmcp 3.x)
cargo run -p mcp-hdm      # HDM (Selafin/SWMM/2dm) MCP server over stdio (rmcp 3.x)
cargo run -p mcp-mesh     # 2D/3D mesh MCP server over stdio (rmcp 3.x)
```

`cadctl` inspects IFC files (STEP header + entity class counts) in a single streaming pass,
without any CAD library - useful for a quick "what is this file?" answer before starting a
server. `cad-sandbox` is the Rust twin of the Python sandbox and is shared by all Rust crates.

## Tool design and security

- Compact, deterministic, context-safe responses; large data is paginated (limit cap: 200).
- Read-only tools are separate from mutating tools; mutators (`dxf_export_slice`, `gis_export_bbox`,
  `hdm_export_slice`, `mesh_slice_bbox`, `mesh_improve_flips`) always write a **new** file under
  `<workspace>/outputs` and return a structured diff - never modify inputs.
- All paths resolve inside `MCP_CAD_WORKSPACE_ROOT`; `..` traversal, outside-absolute paths and
  symlink escapes are rejected with machine-readable error codes.
- Logging goes to **stderr only**; stdout is reserved for the MCP stdio transport.

Details: [`docs/architecture.md`](docs/architecture.md),
[`docs/tool-design.md`](docs/tool-design.md), [`docs/security.md`](docs/security.md).

## CI

`.github/workflows/ci.yml` runs ruff + pytest (Python) and cargo check/clippy/test (Rust)
on every push to `main` and on every PR.

## Commit conventions

Development in this repository is AI-assisted. Every AI agent signs its commits with its own
trailer, so the history shows who did what:

```
Generated by Mistral Vibe.
Co-Authored-By: Mistral Vibe <vibe@mistral.ai>
```

Other agents use their equivalent trailer (e.g. `Co-Authored-By: Claude ...`). Human commits
carry no trailer. The full agent contract (setup, data policy, rules, tool index):
[`AGENTS.md`](AGENTS.md) - Claude Code is redirected via [`CLAUDE.md`](CLAUDE.md).

## License

GPL-3.0-or-later - see [`LICENSE`](LICENSE). Copyleft: derivatives of this project must stay open source under the GPL.
