# mcp-dxf

MCP server for 2D/3D ASCII DXF drawings (pure Rust + rmcp, sandboxed, AI-to-AI envelopes).

Tools (all return `cad-envelope` responses):

| Tool | Kind | Purpose |
| --- | --- | --- |
| `dxf_summary` | read-only | Release, `$INSUNITS`, layer/entity counts, 3D extents, Z=0 audit, LibreDWG repair warnings |
| `dxf_query_layers` | read-only | Paginated layer listing |
| `dxf_query_entities` | read-only | Paginated entity listing filtered by layer, type, elevation, 2D bbox |
| `dxf_extract_text` | read-only | Paginated cleaned TEXT/MTEXT/ATTRIB annotations |
| `dxf_export_slice` | mutating | Lossless slice of entities into a new DXF under `<root>/outputs` |
| `dxf_to_mesh` | mutating | 3DFACE → welded `.2dm`/`.obj` mesh under `outputs/` |
| `dxf_centerline` | mutating | 3D centreline with arc-length pairing and width/crossing QA |
| `dxf_labels_to_3d` | mutating | 2D lines + height labels → 3D DXF under `outputs/` |

Run (stdio):

```bash
MCP_CAD_WORKSPACE_ROOT=/abs/path/to/workspace cargo run -p mcp-dxf
```

Tests: `cargo test -p mcp-dxf`. Contracts: [docs/tool-design.md](../../docs/tool-design.md); sandbox rules: [docs/security.md](../../docs/security.md); agent contract: [AGENTS.md](../../AGENTS.md).
