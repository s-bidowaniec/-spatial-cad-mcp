# ADR 0002. Language boundary: separate stdio processes over a JSON contract (no PyO3 FFI)

**Status** Accepted

**Context** The monorepo mixes Python (`mcp-ifc` on ifcopenshell, shared `mcp_cad_core`) and Rust (four MCP servers plus `cadctl`). Both ecosystems must expose identical tool behavior to AI agents, and agents launch every server as its own process anyway (see `examples/mcp.json`).

**Decision** Each server is a separate stdio process; Python and Rust never share memory. The contract between them is the versioned JSON envelope `schemas/envelope.v1.schema.json` plus the shared sandbox rules, mirrored as `mcp_cad_core` (Python) and `cad-envelope` / `cad-sandbox` (Rust). PyO3/maturin FFI is reserved for a measured hot path inside a single Python process; it is not used today because no current tool call amortizes the conversion and per-platform wheel packaging cost, and separate processes keep each server independently testable, debuggable and deployable.

**Alternatives** A PyO3 extension module wrapping the Rust parsers inside the Python process (fast calls, but wheels per interpreter/platform and a second maintenance surface); calling a Rust CLI via subprocess from Python (same shape as the chosen design, minus the MCP stdio framing); generating shared types from one schema (premature - the envelope schema is the only shared surface and it is already JSON).

**Consequences and verification** `tests/test_envelope_schema.py` validates Python-produced envelopes and Rust golden envelopes against the schema; `cargo test -p cad-envelope -p cad-sandbox` and `tests/test_sandbox.py` keep the twin implementations in lockstep; `tools/check_tools.py` enforces uniform tool metadata across both languages. If a future hot path justifies PyO3, measure end-to-end call time first (not just the inner function) and record the result in a new ADR.
