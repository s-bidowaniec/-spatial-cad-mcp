# mcp-cad-core

Shared core for all `spatial-cad-mcp` servers:

- `mcp_cad_core.sandbox` - strict workspace path sandboxing (`MCP_CAD_WORKSPACE_ROOT`)
- `mcp_cad_core.responses` - standardized JSON ok/error envelopes + `safe_tool` decorator
- `mcp_cad_core.pagination` - deterministic limit/offset paging for large result sets
- `mcp_cad_core.log` - logging to **stderr only** (stdout carries the MCP stdio transport)
