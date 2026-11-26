# Glossary

Project vocabulary mapped to code. Read this when a doc or tool name is unfamiliar.

## Transport and process

| Term | Meaning | Code / record |
| --- | --- | --- |
| envelope | The single JSON response shape of every tool: `{ok, data, meta}` or `{ok: false, error}` | `shared/mcp_cad_core/src/mcp_cad_core/responses.py`, `crates/cad-envelope/src/lib.rs` |
| error code | Stable snake_case branching key in `error.code` | same envelope modules as above |
| stdio transport | MCP over stdin/stdout; stdout is protocol-only, logs go to stderr | `shared/mcp_cad_core/src/mcp_cad_core/log.py`, every `src/main.rs` |
| workspace root | The single trust boundary, configured via `MCP_CAD_WORKSPACE_ROOT` | `shared/mcp_cad_core/src/mcp_cad_core/sandbox.py`, `crates/cad-sandbox/src/lib.rs` |
| sandbox | Path resolution rejecting `..`, outside-absolute paths and symlink escapes | same sandbox modules as above |

## Tools

| Term | Meaning | Code / record |
| --- | --- | --- |
| summary tool | First call on a group (`ifc_summary`, `dxf_summary`, ...): counts, never raw geometry | `packages/mcp-ifc/src/mcp_ifc/tools.py`, `packages/*/src/server.rs` |
| read-only tool | Never writes anywhere, not even caches | [tool-design.md](tool-design.md#4-read-only-vs-mutating-tools) |
| mutating tool | Writes only a new file under `<workspace>/outputs/` and returns a structured diff | `resolve_output` in the sandbox modules |
| outputs | `<workspace>/outputs/` - the only writable location; name collision is an error (`output_exists`) | sandbox `resolve_output` |
| pagination | `limit`/`offset`, hard cap 200, `next_offset`, `limit_clamped_to` | `shared/mcp_cad_core/src/mcp_cad_core/pagination.py`, `crates/cad-envelope/src/lib.rs` |
| truncation | Long values cut with an explicit marker, counted in `meta.truncated_values` | envelope modules |
| model cache | Small stat-guarded LRU of parsed IFC models (keyed by path + mtime/size) | `packages/mcp-ifc/src/mcp_ifc/model_cache.py` |

## Coordination

| Term | Meaning | Code / record |
| --- | --- | --- |
| work item | A planned ticket with a `[KEY]` title and measurable acceptance criteria | [work-item template](../.github/ISSUE_TEMPLATE/work-item.yml) |
| ADR | Architecture Decision Record; the merged ADR decides | [adr/README.md](adr/README.md) |
| repository first | Plans, decisions and evidence live in the repo; Issues are authoritative for scope | [planning/README.md](../planning/README.md) |
| agent trailer | Per-agent `Co-Authored-By` commit trailer | [AGENTS.md](../AGENTS.md#commit-conventions) |
