# cad-envelope

Rust twin of the Python `mcp_cad_core` response envelope, pagination and truncation rules. Every Rust MCP server in this monorepo returns the same `{ok, data, meta}` / `{ok: false, error}` shapes as the Python servers.

- `ok`/`error` envelopes with stable snake_case `code`, `retryable`, `hints`
- deterministic limit/offset pagination (hard cap 200, `limit_clamped_to`, `next_offset`)
- value truncation with explicit markers and `meta.truncated_values`
- `meta.duration_ms` on every response

Tests: `cargo test -p cad-envelope`. Contract: [docs/tool-design.md](../../docs/tool-design.md). Pair with `cad-sandbox`. Agent contract: [AGENTS.md](../../AGENTS.md).
