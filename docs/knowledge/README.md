# Agent knowledge base

What agents learn while working in this repo, recorded here so the next agent starts further along. This is the self-learning layer on top of the durable docs: rules and contracts live in [docs/](../index.md) and [AGENTS.md](../../AGENTS.md); tool-usage patterns, error-code meanings and lessons learned live here.

## Rules

- Add an entry when you learn something reusable: a tool-call sequence that works, an error code you hit and what it actually meant, a performance gotcha, a review finding.
- Synthetic data only: no real model files, client names, credentials or absolute local paths (`tools/check_no_data.py` enforces this in CI).
- Keep entries short and dated (`YYYY-MM-DD`), one topic per file or per section.
- Link new entries from the index below and, when the entry changes how tools are designed, from [docs/index.md](../index.md).
- Write in English, imperative, like [docs/writing-docs.md](../writing-docs.md); links are CI-checked by `tools/check_docs.py`.
- Durable rules discovered this way belong in `docs/` or `AGENTS.md`, not here - move them up when they generalize.

## Index

- [Tool usage patterns](tool-usage.md) - how to drive the five servers well: call order, pagination, mutators, error codes, anti-patterns.
