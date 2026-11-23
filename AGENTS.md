# AGENTS.md - spatial-cad-mcp: MCP servers for CAD / BIM / GIS / HDM, built for AI-to-AI use

For any agent (Claude, GPT/Codex, Mistral, small models) and for humans. Claude Code reads
[CLAUDE.md](CLAUDE.md), which points here; Codex reads this file natively. Short on purpose:
the details live in [docs/](docs/) and the per-package READMEs.

**Status: pre-alpha, MVP across all five tool groups.** `mcp-ifc` (Python) and all four Rust MCP
servers (`mcp-dxf`, `mcp-gis`, `mcp-hdm`, `mcp-mesh`) implement sandboxed, envelope-shaped MVP
tools. Tool names and envelopes may still evolve before the first tagged release.

## Setup

- Python side: uv workspace (Python 3.11+).
  ```bash
  uv sync --all-packages
  uv run pytest; uv run ruff check . && uv run ruff format --check .
  ```
- Rust side: cargo workspace (edition 2024, Rust 1.88+).
  ```bash
  cargo check --workspace; cargo test --workspace
  cargo run -p cadctl -- inspect path/to/model.ifc --top 10
  ```
- Every MCP server is stdio-only and refuses to start without `MCP_CAD_WORKSPACE_ROOT`.
  No tool can touch anything outside that root (see [docs/security.md](docs/security.md)).
- Client config (any MCP host): copy from [examples/mcp.json](examples/mcp.json);
  sample agent prompts: [examples/prompts/agent-prompts.md](examples/prompts/agent-prompts.md).

## Data policy (strict)

Code, tests and synthetic fixtures only. Never copy real project data, client names, survey
files, renders or local absolute paths into the repo or its examples; tests build inputs in
`tmp_path`. Real CAD/BIM/GIS model data lives outside the repo, under the workspace root the
operator configures. Outputs of mutating tools go to `<workspace>/outputs`, never into the repo.

## Answers: one envelope, machine-readable codes

Every tool answers with the standard envelope:

- Success: `{"ok": true, "data": {...}, "meta": {"tool": ..., "pagination": {...}}}`
- Failure: `{"ok": false, "error": {"code", "message", "retryable", ...}}`

Base error codes: `workspace_not_configured`, `path_traversal`, `path_outside_workspace`,
`file_not_found`, `invalid_input`, `cannot_open_ifc` (or format-specific equivalents),
`not_implemented` (scaffold tools), `internal_error` (`retryable: true`).

Lists are bounded (limit cap 200, `meta.limit_clamped_to` when clamped); long values are
truncated. Narrow the query or paginate instead of asking for raw dumps.

## Rules for agents

1. Orient first: call the group's summary tool (`ifc_summary`, later `dxf_summary`,
   `gis_summary`, `hdm_summary`, `mesh_summary`) before any other call, and report counts
   before details.
2. Never pull raw geometry or full entity trees into your context. Use the paginated query
   tools; fetch more pages only when the task needs them.
3. Read-only tools are separate from mutating tools. Mutators (`dxf_export_slice`, `dxf_edit_entities`, `dxf_create`, `dxf_add_entities`, `dxf_to_mesh`,
   `dxf_centerline`, `dxf_labels_to_3d`, `gis_export_bbox`, `hdm_export_slice`, `mesh_slice_bbox`,
   `mesh_improve_flips`, `mesh_cut_plane`, `mesh_volumes_intersect` with `output_dxf_name`,
   `mesh_to_dxf`) always write a **new** file under `<workspace>/outputs` and return a structured
   diff - inputs are never modified.
4. All file arguments are relative to `MCP_CAD_WORKSPACE_ROOT`. `..` traversal,
   outside-absolute paths and symlink escapes are rejected; do not try to work around the
   sandbox, and do not loosen it to make a call pass.
5. Verify before claiming success: `uv run pytest`, `ruff check`, `cargo check`/`clippy`/`test`
   and the CI workflow must stay green after your change. Report failing checks honestly; do
   not "fix" a failing test by loosening it.
6. Keep the envelope vocabulary consistent across Python and Rust implementations
   (see [docs/tool-design.md](docs/tool-design.md)).
7. Do not guess file contents: IFC/DXF/GIS/HDM files can be hundreds of MB - always confirm
   through the summary/query tools, never by reading the raw file into the conversation.

## Tool index

| Group | Package | Language | Tools (bold = implemented) |
| --- | --- | --- | --- |
| BIM | `packages/mcp-ifc` | Python (ifcopenshell) | **ifc_summary**, **ifc_query_elements**, **ifc_get_psets**, **ifc_validate_required_props** |
| CAD | `packages/mcp-dxf` | Rust (pure Rust + rmcp) | **dxf_summary**, **dxf_query_layers**, **dxf_query_entities**, **dxf_extract_text**, **dxf_export_slice**, **dxf_edit_entities**, **dxf_create**, **dxf_add_entities**, **dxf_to_mesh**, **dxf_centerline**, **dxf_labels_to_3d** |
| GIS | `packages/mcp-gis` | Rust (rusqlite + rmcp) | **gis_summary**, **gis_extract_schema**, **gis_query_features**, **gis_query_point**, **gis_export_bbox** |
| HDM | `packages/mcp-hdm` | Rust (pure Rust + rmcp) | **hdm_summary**, **hdm_query_timeseries**, **hdm_query_profiles**, **hdm_export_slice** |
| mesh | `packages/mcp-mesh` | Rust (pure Rust + rmcp) | **mesh_summary**, **mesh_query_parts**, **mesh_query_quality**, **mesh_sample_elevation**, **mesh_section_line**, **mesh_diff**, **mesh_slice_bbox**, **mesh_improve_flips**, **mesh_cut_plane**, **mesh_volumes_intersect**, **mesh_to_dxf** |
| CLI | `crates/cadctl` | Rust | inspect: IFC header + entity counts (no CAD libs) |

Roadmaps for the scaffold groups: [docs/architecture.md](docs/architecture.md).

## Commit conventions

Development is AI-assisted. **Every AI agent signs its commits with its own trailer**, so the
history shows who did what - the same convention as `s-bidowaniec/DroidOps`:

```
Co-Authored-By: Mistral Vibe <noreply@mistral.ai>
```

Other agents use their equivalent trailer (e.g. `Co-Authored-By: Claude ...`,
`Co-Authored-By: GPT-6 Codex ...`); use every trailer of every agent that materially shaped the
commit. Human commits carry no trailer. Test and docs commits follow the same rule.

## Maintaining these docs

- New MCP tool: implement the plain function + tests first, then the FastMCP/rmcp wrapper with
  an identical name, then update this index, the package README and [docs/tool-design.md](docs/tool-design.md).
- New package: mirror the existing scaffold layout (Cargo.toml inheriting workspace fields,
  `src/main.rs` stdio entry, `src/server.rs` with tools), keep the section here in sync.
- The sandbox rules in [docs/security.md](docs/security.md) are binding: changes require
  reviewer attention in the PR description.
