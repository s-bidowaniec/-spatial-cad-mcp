# Documentation index

Route each task to its guide, package, tests and decision records. Read this when you start a ticket in this repo; the [glossary](glossary.md) maps project terms to code.

[Onboarding](onboarding.md) · [Contributing](../CONTRIBUTING.md) · [Planning](../planning/README.md) · [ADR 0001](adr/0001-repository-first-coordination.md)

## Area → sources

| Area | Package / crate | Guide | Tests and checks |
| --- | --- | --- | --- |
| BIM / IFC | `packages/mcp-ifc` (`packages/mcp-ifc/src/mcp_ifc/server.py`) | [tool-design](tool-design.md) | `tests/test_ifc_tools.py` |
| CAD / DXF | `packages/mcp-dxf` (`packages/mcp-dxf/src/server.rs`) | [tool-design](tool-design.md) | `cargo test -p mcp-dxf` |
| GIS | `packages/mcp-gis` (`packages/mcp-gis/src/server.rs`) | [tool-design](tool-design.md) | `cargo test -p mcp-gis` |
| HDM | `packages/mcp-hdm` (`packages/mcp-hdm/src/server.rs`) | [tool-design](tool-design.md) | `cargo test -p mcp-hdm` |
| Mesh | `packages/mcp-mesh` (`packages/mcp-mesh/src/server.rs`) | [tool-design](tool-design.md) | `cargo test -p mcp-mesh` |
| Shared Python core (envelope, pagination, sandbox, logging) | `shared/mcp_cad_core` (`shared/mcp_cad_core/src/mcp_cad_core/responses.py`, `shared/mcp_cad_core/src/mcp_cad_core/sandbox.py`) | [architecture](architecture.md), [security](security.md) | `tests/test_responses.py`, `tests/test_sandbox.py` |
| Rust core twins | `crates/cad-envelope` (`crates/cad-envelope/src/lib.rs`), `crates/cad-sandbox` (`crates/cad-sandbox/src/lib.rs`) | [security](security.md) | `cargo test -p cad-envelope -p cad-sandbox` |
| CLI | `crates/cadctl` (`crates/cadctl/src/main.rs`) | [architecture](architecture.md) | `cargo test -p cadctl` |
| Repo hygiene | `tools/check_docs.py`, `tools/check_no_data.py` | [governance](github-governance.md), [engineering](engineering.md) | `tests/test_check_docs.py`, `tests/test_check_no_data.py`, CI: `.github/workflows/ci.yml` |

## Decisions and process

- [ADR index](adr/README.md) - accepted decisions and proposals awaiting review.
- [Envelope contract](../schemas/envelope.v1.schema.json) - the cross-language JSON Schema every tool result validates against (`tests/test_envelope_schema.py`).
- [Agent knowledge base](knowledge/README.md) - tool-usage patterns and lessons learned, written by agents while working here.
- [GitHub governance](github-governance.md) - issue/PR forms, label taxonomy, merge rules.
- [Writing docs](writing-docs.md) - doc style and link rules, enforced by `tools/check_docs.py` in CI.
- [Onboarding: run the checks](onboarding.md#run-the-checks) - the commands CI runs on every PR.

## Client-facing

- `examples/mcp.json` - MCP host configuration; `examples/prompts/agent-prompts.md` - sample agent prompts.
- `llms.txt` - machine-readable index for agents arriving at this repo from outside.
- [AGENTS.md](../AGENTS.md) - the agent contract: setup, data policy, envelope, rules, tool index, commit conventions.
