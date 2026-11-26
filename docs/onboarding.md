# Onboarding

spatial-cad-mcp is a pre-alpha monorepo of deterministic MCP servers for CAD / BIM / GIS / HDM / mesh workflows, built for AI-to-AI use. Five tool groups share one JSON envelope and one workspace sandbox: `mcp-ifc` (Python), `mcp-dxf`, `mcp-gis`, `mcp-hdm`, `mcp-mesh` (Rust), plus the `cadctl` CLI. Read [AGENTS.md](../AGENTS.md) first - it is the agent contract.

## Learn the workflow and the decisions

- Start from [Repository First Coordination](../planning/README.md). GitHub Issues are authoritative for scope, owner, acceptance and closure. The repo keeps the linked decisions, status and evidence.
- Read the active [ADRs](adr/README.md), the [architecture](architecture.md) and the [tool design rules](tool-design.md). Every tool speaks the same envelope and the same sandbox rules; mutators write only new files under `<workspace>/outputs`.
- For an interface or project-rule change, open a PR with an [ADR](adr/README.md). Until the owner and the consumers accept it, keep the status `Proposed`. After acceptance, link the change to the contract, the Issue and the evidence.

## Pick up and claim work

1. Read the work item and check the [governance labels](github-governance.md). Check `main`, open PRs and claimed Issues. Separate start conditions from acceptance conditions.
2. Take ownership: assign the Issue (or note the agent session in it), create a branch `feat/<key>-<slug>` or `fix/<key>-<slug>` from the current `main`, and set the `status:` label. Work in the task branch. If claims conflict, record the decision in the repo before implementing.
3. Start a behavior change with a scenario on the public API (a tool call) and an independent expectation. Add the synthetic case to Git - tests build inputs in `tmp_path`. Real model data and access-restricted files stay outside Git and GitHub.

## Run the checks

From the repository root, Python 3.11+ and Rust 1.88+:

```bash
uv sync --all-packages
uv run pytest
uv run ruff check . && uv run ruff format --check .
cargo fmt --all -- --check
cargo check --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
python tools/check_docs.py      # links, anchors, cited paths, AGENTS.md budget
python tools/check_no_data.py   # data policy: no real model files in Git
python tools/check_tools.py     # tool definitions: names, descriptions, annotations
```

Or run the whole gate at once: `bash tools/check_all.sh` (same set, CI runs it in two jobs).

CI runs the same set on every push to `main` and every PR (`.github/workflows/ci.yml`). Acceptance evidence requires explicitly running the relevant subset and checking its expectations.

## Hand off the change

Open a PR per [CONTRIBUTING.md](../CONTRIBUTING.md), [AGENTS.md](../AGENTS.md) and the [engineering standards](engineering.md). Point to the Issue, the base, the contract impact, the commands run and their results. If assumptions, dependencies or evidence changed, update the plan docs in the same PR. After the evidence is accepted, release the claim and link the decision or result to the Issue.
