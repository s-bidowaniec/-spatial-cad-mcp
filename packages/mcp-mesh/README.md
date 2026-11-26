# mcp-mesh

MCP server for 2D/3D mesh inspection, topology, quality QA and editing (pure Rust + rmcp, sandboxed, AI-to-AI envelopes). Supported formats: SMS `.2dm`, Wavefront `.obj`, ASCII & binary `.stl`.

Tools (all return `cad-envelope` responses):

| Tool | Kind | Purpose |
| --- | --- | --- |
| `mesh_summary` | read-only | Counts, 3D bbox, watertightness, boundary/non-manifold edges, Euler characteristic, aggregate QA |
| `mesh_query_parts` | read-only | Paginated part/group/material zone summaries |
| `mesh_query_quality` | read-only | Paginated worst/failing elements by angle, area ratio, validity, bbox |
| `mesh_sample_elevation` | read-only | Barycentric elevation sampling on the mesh surface |
| `mesh_section_line` | read-only | Cross-section along a polyline |
| `mesh_diff` | read-only | Vertex/face deltas between two meshes (XY-matched dz stats) |
| `mesh_slice_bbox` | mutating | New `.obj`/`.2dm` slice under `<root>/outputs` |
| `mesh_improve_flips` | mutating | Greedy interior diagonal-flip grading, boundaries preserved, under `outputs/` |
| `mesh_cut_plane` | mutating | Exact plane cut → chained 3D DXF polylines under `outputs/` |
| `mesh_volumes_intersect` | mutating | Exact cut/fill volumes + 3D zero-line overlay under `outputs/` |
| `mesh_to_dxf` | mutating | 3DFACE per part + 3D boundary loops DXF under `outputs/` |

Run (stdio):

```bash
MCP_CAD_WORKSPACE_ROOT=/abs/path/to/workspace cargo run -p mcp-mesh
```

Tests: `cargo test -p mcp-mesh`. Contracts: [docs/tool-design.md](../../docs/tool-design.md); sandbox rules: [docs/security.md](../../docs/security.md); agent contract: [AGENTS.md](../../AGENTS.md).
