# cad-sandbox

Workspace path sandboxing for the Rust side; mirrors `mcp_cad_core.sandbox` so every binary rejects paths identically:

1. Reject any relative path with a `..` component (`path_traversal`).
2. Reject absolute paths outside the workspace root (`path_outside_workspace`).
3. Reject symlink escapes: existing components are canonicalized before the containment check (`path_outside_workspace`).
4. Mutating writes go to *new* files under `<root>/outputs` only; collisions are refused (`output_exists`).

Servers refuse to start without `MCP_CAD_WORKSPACE_ROOT`.

Tests: `cargo test -p cad-sandbox`. Threat model: [docs/security.md](../../docs/security.md). Agent contract: [AGENTS.md](../../AGENTS.md).
