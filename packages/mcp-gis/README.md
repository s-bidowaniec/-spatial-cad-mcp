# mcp-gis

MCP server for GIS vector datasets: GeoPackage (`.gpkg`, via `rusqlite` + WKB decoder) and native GeoJSON (pure Rust + rmcp, sandboxed, AI-to-AI envelopes).

Tools (all return `cad-envelope` responses):

| Tool | Kind | Purpose |
| --- | --- | --- |
| `gis_summary` | read-only | Format, CRS/EPSG, layers, geometry types, feature counts, extents, CRS-vs-coordinate sanity warnings |
| `gis_extract_schema` | read-only | Per-layer attribute schema with null rates |
| `gis_query_features` | read-only | Paginated feature summaries filtered by layer, bbox and attributes (never raw geometry) |
| `gis_query_point` | read-only | Exact point-in-polygon with hole rings + radius search, paginated |
| `gis_export_bbox` | mutating | Clips features intersecting a 2D bbox into a new `.geojson` under `<root>/outputs` with a structured diff |

Run (stdio):

```bash
MCP_CAD_WORKSPACE_ROOT=/abs/path/to/workspace cargo run -p mcp-gis
```

Tests: `cargo test -p mcp-gis`. Contracts: [docs/tool-design.md](../../docs/tool-design.md); sandbox rules: [docs/security.md](../../docs/security.md); agent contract: [AGENTS.md](../../AGENTS.md).
