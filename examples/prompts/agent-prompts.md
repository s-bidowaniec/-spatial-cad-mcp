# Sample agent prompts

Prompts that pair well with the mcp-ifc server. They follow the intended call order:
orient (`ifc_summary`) -> slice (`ifc_query_elements`) - inspect (`ifc_get_psets`) - audit
(`ifc_validate_required_props`), so the agent never needs the raw IFC tree in context.

## Prompt 1 - Model orientation

```text
Connect to the mcp-ifc server (workspace root is already configured).

The file model.ifc sits at the root of the workspace. Give me a project brief:
1. Call ifc_summary and report schema, length unit and the top element classes.
2. Call ifc_query_elements for IfcSpace with limit 25, and summarize how the
   spaces are named (any numbering pattern?).
3. Pick 3 representative spaces and compare their property sets with ifc_get_psets.
Do not fetch more pages than needed. Answer with a table and cite GlobalIds.
```

## Prompt 2 - Completeness audit (deterministic QA)

```text
Using mcp-ifc, audit model.ifc for missing data quality:
1. ifc_validate_required_props for all IfcWall elements requiring the properties
   ["LoadBearing", "IsExternal", "FireRating"] scoped to pset Pset_WallCommon.
2. Then audit IfcDoor with the same call, requiring ["FireRating"] in Pset_DoorCommon.
3. Summarize: how many elements pass, how many have issues, and list up to 10 offending
   GlobalIds with the exact missing properties. Suggest a prioritized fix order.
```

## Prompt 3 - Targeted property dig

```text
In model.ifc find every element whose Name matches the pattern ".*[Ss]troje.*".
For each match (use ifc_query_elements with the regex), call ifc_get_psets and report
only properties whose names contain "Load" or "Mass". Paginate with limit 50.
Finish with a one-paragraph summary of the structural framing inventory.
```

## Anti-pattern (what NOT to do)

```text
Dump all entities and attributes of model.ifc.
```

Tools intentionally cannot do this: responses are compact and paginated to protect the
caller's context window. See docs/tool-design.md for the design rationale.
