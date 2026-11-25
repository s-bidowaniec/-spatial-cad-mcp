## Work item and scope

<!-- Link the Issue / work-item key, the base commit, and the status label. Use Closes #… when the evidence completes the Issue. -->

## Contracts and compatibility

<!-- Identify envelope / error-code changes, tool renames, sandbox-rule changes, affected packages and consumers. Link the ADR for shared-contract changes. -->

## Verification

<!-- Record exact commands and results: uv run pytest, ruff check / ruff format --check ., cargo check / clippy / test --workspace, python tools/check_docs.py, python tools/check_no_data.py. Include OS and tool versions. Distinguish rejection codes that were exercised. -->

## Handoff

- [ ] Updated the Issue status and its `status:` label (see docs/github-governance.md), or this PR itself records the claim.
- [ ] Synced the docs affected by the change: AGENTS.md tool index, package README, docs/index.md / docs/tool-design.md.
- [ ] Reviewed failure modes, resource bounds (pagination cap, truncation) and compatibility for the changed surface.
- [ ] Kept real model data, credentials, client names and local absolute paths outside the repo (tools/check_no_data.py is green).
