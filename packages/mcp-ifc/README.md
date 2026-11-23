# mcp-ifc

Read-only MVP MCP server for IFC models (BIM), built on `ifcopenshell`.

Tools (all deterministic, all returning `mcp_cad_core` response envelopes):

| Tool | Kind | Purpose |
| --- | --- | --- |
| `ifc_summary` | read-only | Schema, units, total counts, element counts by IFC class |
| `ifc_query_elements` | read-only | Paginated element listing, filter by IFC class and/or name pattern |
| `ifc_get_psets` | read-only | Property sets and properties for one element (by GlobalId) |
| `ifc_validate_required_props` | read-only | Audit a class for elements missing required properties |
| `ifc_element_bbox` | read-only | World bbox (min/max), center, and triangle count for one element; optional children union (cap 200) |
| `ifc_geometry_summary` | read-only | Per-class aggregates: count, bbox union, smallest-footprint element; paginated (cap 200) |

Geometry tools (`ifc_element_bbox`, `ifc_geometry_summary`) use the OCCT kernel
(`ifcopenshell.geom`) and never return raw geometry - only bounding boxes, centers,
triangle counts, and per-class aggregates. Without the kernel they answer
`not_implemented` ("geometry kernel unavailable; install ifcopenshell[occ]").
Shape summaries are cached per (file, GlobalId) with a TTL (see `model_cache.py`).

Run:

```bash
MCP_CAD_WORKSPACE_ROOT=/abs/path/to/workspace uv run --project packages/mcp-ifc mcp-ifc
```
