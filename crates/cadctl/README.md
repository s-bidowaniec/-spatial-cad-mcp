# cadctl

Fast, dependency-light CLI for AI agents working with CAD files inside a sandboxed workspace. Compact JSON envelopes on stdout, diagnostics on stderr.

- `cadctl inspect <file.ifc> [--top N]` - streaming IFC (ISO 10303-21) header schema + entity class counts, no CAD library, constant memory
- `cadctl sandbox-check <path> [--root DIR]` - dry-run: does a path stay inside the workspace root (defaults to `MCP_CAD_WORKSPACE_ROOT`)

```bash
MCP_CAD_WORKSPACE_ROOT=/abs/path/to/workspace cargo run -p cadctl -- inspect model.ifc --top 10
```

Tests: `cargo test -p cadctl`. Agent contract: [AGENTS.md](../../AGENTS.md); sandbox rules: [docs/security.md](../../docs/security.md).
