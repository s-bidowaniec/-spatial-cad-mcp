# Contributing to spatial-cad-mcp

Start from the [onboarding guide](docs/onboarding.md) and [Repository First Coordination](planning/README.md#repository-first-coordination).

Before implementing, confirm the work item's scope, dependencies and acceptance criteria in its Issue (use the [work-item template](.github/ISSUE_TEMPLATE/work-item.yml)). Record the claim in the Issue and set the `status:` label per [GitHub governance](docs/github-governance.md).

## Before opening a PR

- Update the work status in the Issue and its `status:` label. In the PR description give the Issue ID / work-item key, the base commit and the evidence.
- Describe the impact on the envelope contract, consumers and compatibility. A change to a shared contract (envelope, error codes, sandbox rules, tool names) must be paired with a proposed or accepted [ADR](docs/adr/README.md). Update [docs/index.md](docs/index.md) when the documentation map changes.
- Record the results of `uv run pytest`, `ruff check` / `ruff format --check .`, `cargo check` / `cargo clippy --workspace --all-targets` / `cargo test --workspace`, `python tools/check_docs.py` and `python tools/check_no_data.py`. Explain how the failure mode was verified (which rejection code, which sandbox escape was refused).
- If the PR closes the work item on merge, link it with the keyword `Closes #…`. If it delivers only part, use a plain reference and record the remaining gate. Release the claim after the evidence is accepted.

Never put real model data, client names, survey files or local absolute paths into the repo. Encode the problem as a synthetic regression built in `tmp_path`. See the [data policy](AGENTS.md#data-policy-strict) and the [engineering standards](docs/engineering.md).
