# Security model

## Threat model

The MCP servers execute file operations requested by an autonomous caller (an agent). The
primary risks:

1. **Path escape**: a prompt-injected or hallucinated tool argument naming files outside the
   intended workspace (`/etc/passwd`, `../../secrets.key`, absolute paths).
2. **Symlink escape**: a workspace file that is a symlink pointing outside the root.
3. **Destructive writes**: overwriting input files or other agents' outputs.
4. **Protocol corruption**: stray stdout output breaking the stdio JSON-RPC stream.
5. **Resource exhaustion**: unbounded parsing or unbounded responses.

## Mitigations

### Workspace sandbox

- Exactly one trust boundary: the directory in `MCP_CAD_WORKSPACE_ROOT`. The server refuses
  to start when the variable is unset or points to a non-directory (fail fast, exit code 2).
- Path resolution (`mcp_cad_core.sandbox` in Python, `cad-sandbox` in Rust) rejects:
  - any `..` component (including Windows-style separators, normalized before checking):
    error `path_traversal`;
  - absolute paths outside the root: `path_outside_workspace`;
  - paths whose *existing* components are symlinks escaping the root: resolution
    canonicalizes before the containment check: `path_outside_workspace`.
- The check is enforced **inside** every tool and CLI command; no tool takes an
  unrestricted filesystem path.

### Writes

- The MVP servers are 100% read-only.
- Mutating tools (roadmap) write only to **new** files under `<root>/outputs/`:
  `resolve_output` refuses existing names (`output_exists`) and any name containing a
  directory component (`path_traversal`). Overwrites are structurally impossible, not
  merely discouraged.
- Rationale: an agent's session stays reproducible and auditable, and a bad prompt can never
  damage the source model.

### Process hygiene

- stdio transport: **stdout is protocol-only**. Logs go to stderr in every server
  (`mcp_cad_core.log` configures a stderr handler; Rust uses stderr too).
- Structured in-band errors instead of exceptions keep failure data bounded.

### Resource bounds

- Responses are paginated (hard cap 200/page), string values truncated, JSON-oriented.
- The Python servers keep at most 3 parsed models in the LRU cache; loading is stat-guarded.
- `cadctl` streams files line-by-line with constant memory (no full-file buffer).

## Non-goals

- No sandboxing against the process itself (a run is trusted with CPU/RAM within the host's
  limits); sandboxing the *filesystem scope* only.
- No network egress from any server or tool; nothing planned.

## Verification

- `tests/test_sandbox.py` covers traversal, absolute escapes, symlink escapes, output
  overwrite attempts and misconfiguration codes for the Python sandbox.
- `crates/cad-sandbox` carries the equivalent Rust unit test suite (`cargo test -p
  cad-sandbox`).
- CI runs both suites on every push and PR.
