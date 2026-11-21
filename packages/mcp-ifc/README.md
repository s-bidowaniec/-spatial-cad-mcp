# mcp-ifc

Read-only MVP MCP server for IFC models (BIM), built on `ifcopenshell`.

Tools (all deterministic, all returning `mcp_cad_core` response envelopes):

| Tool | Kind | Purpose |
| --- | --- | --- |
| `ifc_summary` | read-only | Schema, units, total counts, element counts by IFC class |
| `ifc_query_elements` | read-only | Paginated element listing, filter by IFC class and/or name pattern |
| `ifc_get_psets` | read-only | Property sets and properties for one element (by GlobalId) |
| `ifc_validate_required_props` | read-only | Audit a class for elements missing required properties |

Run:

```bash
MCP_CAD_WORKSPACE_ROOT=/abs/path/to/workspace uv run --project packages/mcp-ifc mcp-ifc
```
