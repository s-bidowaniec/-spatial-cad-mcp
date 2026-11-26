# mcp-hdm

MCP server for open-standard hydrodynamic model files (pure Rust + rmcp, sandboxed, AI-to-AI envelopes).

Supported formats: TELEMAC Selafin (`.slf`/`.res`), EPA SWMM (`.inp`), SMS `.2dm`.

Tools (all return `cad-envelope` responses):

| Tool | Kind | Purpose |
| --- | --- | --- |
| `hdm_summary` | read-only | Format, title, node/element counts, extent, time window, available variables |
| `hdm_query_timeseries` | read-only | Paginated time series by variable + node/element, with min/max/mean |
| `hdm_query_profiles` | read-only | Selafin 2D line profiles + SWMM conduit profiles |
| `hdm_export_slice` | mutating | CSV export of the selected slice under `<root>/outputs` |

Run (stdio):

```bash
MCP_CAD_WORKSPACE_ROOT=/abs/path/to/workspace cargo run -p mcp-hdm
```

Tests: `cargo test -p mcp-hdm`. Contracts: [docs/tool-design.md](../../docs/tool-design.md); sandbox rules: [docs/security.md](../../docs/security.md); agent contract: [AGENTS.md](../../AGENTS.md).
