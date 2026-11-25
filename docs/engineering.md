# Engineering standards

Execution and handoff follow [Repository First Coordination](../planning/README.md). The envelope contract and sandbox rules in [AGENTS.md](../AGENTS.md), [tool-design.md](tool-design.md) and [security.md](security.md) are binding.

## Stack, types and configuration

- Python 3.11+ in a uv workspace (`shared/mcp_cad_core`, `packages/mcp-ifc`); `ruff check` + `ruff format --check .` and `pytest` cover the Python side. Rust workspace, edition 2024, Rust 1.88+ (`cargo fmt`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`).
- Use `pathlib.Path` for files; keep I/O at the edges and computations pure. Prefer typed returns and explicit error envelopes over exceptions crossing the tool boundary (`safe_tool`).
- Name identifiers and fields with units or frames where applicable (`clearance_m`, `xyz_local`). Keep parameter and tool names stable forever - renames are breaking changes.

## Response envelope and diagnostics

- Every tool answers with the one envelope: `{ok: true, data, meta}` or `{ok: false, error: {code, message, retryable, ...}}`. `code` values are stable snake_case; the Python (`mcp_cad_core.responses`) and Rust (`cad-envelope`) twins must stay in sync.
- stdout carries only protocol frames; logs go to stderr. Serializers must not emit NaN/Infinity.
- Lists are paginated: hard cap 200 per page, `limit_clamped_to` when clamped, `next_offset` when more remain. Long string values are truncated with an explicit marker and counted in `meta.truncated_values`. Include `duration_ms` in `meta`.
- Behavior is deterministic: same arguments, same envelope shape (modulo `meta` timings); explicit stable sort keys.

## Sandbox and writes

- One trust boundary: `MCP_CAD_WORKSPACE_ROOT`. Reject `..` components, outside-absolute paths and symlink escapes with machine-readable codes (`path_traversal`, `path_outside_workspace`). Never loosen the sandbox to make a call pass.
- Read-only tools write nowhere. Mutating tools write only a new file under `<workspace>/outputs/` (`output_exists` on collision) and return a structured diff. Inputs are never modified.

## Data policy

- Code, tests and synthetic fixtures only. Tests build inputs in `tmp_path`. No real model files, client names or local absolute paths in Git - enforced by `python tools/check_no_data.py` in CI.
- Keep the docs honest: relative links, anchors and the cited paths in `docs/index.md` are verified by `python tools/check_docs.py`; keep `AGENTS.md` within its line budget.

## Performance

- Measure before optimizing; record input size, counts and timings next to the change. Keep responses bounded (pagination, truncation) and caches stat-guarded (see `model_cache.py`).

## Commits

- AI-assisted development: every AI agent signs its commits with its own `Co-Authored-By` trailer (see [AGENTS.md](../AGENTS.md#commit-conventions)); human commits carry no trailer.
