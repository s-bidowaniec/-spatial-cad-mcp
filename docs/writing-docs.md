# Writing documentation

How to write and link docs in this repo. Docs are read by AI agents first - keep them short, imperative and machine-checkable.

## Rules

- English, concise, imperative ("Add a test", not "You should add a test"). One topic per file.
- Start every doc with an H1 title and a one-paragraph summary: what the doc is and when to read it.
- Link liberally with relative links, e.g. [tool design](tool-design.md). Every relative link, anchor and cited path in `docs/index.md` is verified by [tools/check_docs.py](../tools/check_docs.py) in CI - run it before pushing.
- Use tables for mappings (term → code, area → package → tests), checklists for process steps, fenced code blocks with exact copy-pasteable commands.
- Cite real files as code spans in `docs/index.md` (`packages/mcp-dxf/src/server.rs`); the checker verifies they exist. Never use ellipses inside a path code span.
- Never put real model data, credentials, client names or absolute local paths into docs. Describe synthetic fixtures built in `tmp_path`.
- Update docs in the same PR as the code they describe: the AGENTS.md tool index, the package README, the `docs/index.md` map.
- Keep `AGENTS.md` within its line budget (default 120, enforced by `tools/check_docs.py`): the root file is the navigation hub; details live in `docs/`.

## Where things live

| Content | Place |
| --- | --- |
| Agent contract (setup, rules, tool index, commits) | `AGENTS.md` at the root; thin pointers in `CLAUDE.md` and `.github/copilot-instructions.md` |
| Onboarding and check commands | [onboarding.md](onboarding.md) |
| Coding and engineering standards | [engineering.md](engineering.md), [tool-design.md](tool-design.md), [security.md](security.md) |
| Navigation map (area → guide → package → tests) | [index.md](index.md) |
| Decisions | [adr/](adr/README.md) |
| Coordination and governance | [planning/README.md](../planning/README.md), [github-governance.md](github-governance.md), [CONTRIBUTING.md](../CONTRIBUTING.md) |
| Per-package usage (tools, run, tests) | `packages/*/README.md`, `crates/*/README.md`, `shared/*/README.md` |
| Machine-readable entry for external agents | `llms.txt` at the root |
