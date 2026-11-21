# Tool design guidelines (AI-to-AI)

These rules govern every tool added to this monorepo. They exist because tool *selection* and
tool *output* are consumed by LLMs whose context is expensive and finite.

## 1. Tool surfaces

- **Small, orthogonal, named for intent**: `ifc_summary`, `ifc_query_elements` - an agent
  should be able to pick tools from the name alone.
- Docstrings are part of the product. Structure them as: one-sentence purpose (what it
  returns, not how), when to prefer this tool, related tools for follow-up, then `Args:` with
  per-parameter guidance (units, defaults, bounds, examples). The MCP SDK derives the JSON
  Schema descriptions from exactly this text.
- Keep parameter names stable forever; treat renames as breaking API changes.

## 2. Context budget

- Never return a raw file tree or geometry. Return **summaries with counts**, and paginated
  slices for anything that can exceed ~25 items.
- Hard page cap (200) with `next_offset` metadata; never silently return more.
- Cap noisy strings: long property values are truncated with an explicit marker and a
  `truncated_values` count in `meta`.
- Include `duration_ms` in `meta` so agents can reason about cost.

## 3. Response envelope

Always the same two shapes (see `mcp_cad_core.responses`):

```json
{ "ok": true,  "data": {}, "meta": {} }
{ "ok": false, "error": { "code": "...", "message": "...", "retryable": false } }
```

- `code` is the branching key - stable, snake_case, e.g. `path_outside_workspace`,
  `element_not_found`, `invalid_input`, `cannot_open_ifc`, `not_implemented`.
- `message` must state what to do next, not just what failed.
- `hints` (optional list) suggests concrete follow-up tool calls.
- In-band beats protocol-level errors: prefer returning an envelope over raising, so the
  agent sees structured failure instead of an opaque transport error. The `safe_tool`
  decorator makes this the default: a tool **cannot** leak an exception as a raw error.

## 4. Read-only vs mutating tools

- Every tool docstring states its kind explicitly (`read-only` / `mutating`).
- Read-only tools must not write anywhere - not even "harmless" caches inside the workspace.
- Mutating tools (roadmap) never modify input files. They always write a **new** file under
  `<workspace>/outputs/` (name collision = error, `output_exists`), and return a structured
  diff/summary: what changed, counts before/after, output path.
- This discipline means an entire agent session is reproducible: inputs are immutable, outputs
  are additive and inspectable.

## 5. Determinism

- Same arguments, same envelope (modulo `meta` timings). No wall-clock, randomness or
  iteration-order leaks in `data`.
- Ordering is explicit: stable sort keys (e.g. class name), documented in the docstring.

## 6. Checklists for new tools

Adding a tool to a Python server:

1. Implement a plain function in `tools.py`, decorated with `@safe_tool`, with a docstring
   per rules above; resolve paths via `workspace_sandbox()`, load via the model cache.
2. Expose it in `server.py` with an MCP-facing wrapper (keeps the plain function testable).
3. Add tests: happy path, every rejection code, pagination bounds, `safe_tool` behavior.
4. `ruff check` + `ruff format` must pass.

Adding a tool to a Rust server (`rmcp` 3.x):

1. Define a `XxxParams` struct (`serde::Deserialize` + `schemars::JsonSchema`); document
   fields with doc comments - field docs, not struct docs, end up in the schema.
2. Add the `#[tool(description = "...")]` method to the `#[tool_router(server_handler)]`
   impl block; return compact JSON strings using the same envelope vocabulary. Resolve paths
   through `cad_sandbox::WorkspaceSandbox`.
3. Cover rejection paths with `cargo test` cases in the crate.
