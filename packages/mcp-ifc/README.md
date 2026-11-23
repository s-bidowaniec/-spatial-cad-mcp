# mcp-ifc

Read-only MVP MCP server for IFC models (BIM), built on `ifcopenshell`.

Tools (all deterministic, all returning `mcp_cad_core` response envelopes):

| Tool | Kind | Purpose |
| --- | --- | --- |
| `ifc_summary` | read-only | Schema, units, total counts, element counts by IFC class |
| `ifc_query_elements` | read-only | Paginated element listing, filter by IFC class and/or name pattern |
| `ifc_get_psets` | read-only | Property sets and properties for one element (by GlobalId) |
| `ifc_validate_required_props` | read-only | Audit a class for elements missing required properties |
| `ifc_set_property` | **mutating** | Set one property in a Pset on selected elements (class_filter / global_ids cap 200) into a new validated IFC under `outputs/`; returns a per-element old/new changes diff |

`ifc_set_property` requires at least one selector (a selector matching nothing answers
`element_not_found`), writes only a new file under `<workspace>/outputs/` (plain `.ifc`
name; collisions answer `output_exists`), stores numeric values as IfcInteger/IfcReal and
other values as IfcText, and never modifies the input file.

Run:

```bash
MCP_CAD_WORKSPACE_ROOT=/abs/path/to/workspace uv run --project packages/mcp-ifc mcp-ifc
```
