# mcp-ifc

Read-only MVP MCP server for IFC models (BIM), built on `ifcopenshell`.

Tools (all deterministic, all returning `mcp_cad_core` response envelopes):

| Tool | Kind | Purpose |
| --- | --- | --- |
| `ifc_summary` | read-only | Schema, units, total counts, element counts by IFC class |
| `ifc_query_elements` | read-only | Paginated element listing, filter by IFC class and/or name pattern |
| `ifc_get_psets` | read-only | Property sets and properties for one element (by GlobalId) |
| `ifc_validate_required_props` | read-only | Audit a class for elements missing required properties |
| `ifc_export_submodel` | **mutating** | Export selected products (class_filter / global_ids cap 200 / container_global_id) to a new validated IFC under `outputs/`; inputs never modified |

`ifc_export_submodel` requires at least one selector, writes only a new file under
`<workspace>/outputs/` (`output_name` must be a plain `.ifc` name; collisions answer
`output_exists`), keeps header/spatial hierarchy/type objects/materials, re-opens the
written file to validate it, and reports unknown ids in `data.missing_global_ids`.

Run:

```bash
MCP_CAD_WORKSPACE_ROOT=/abs/path/to/workspace uv run --project packages/mcp-ifc mcp-ifc
```
