# Planning and coordination

## Repository First Coordination

The repository is the authoritative place for planning, execution status, architecture decisions, dependencies and agent coordination. Information needed to do the work must land in the repository. If an agreement arose in a spreadsheet, chat, external document or local note, record its content, scope and evidence in the appropriate Issue, ADR or file on `main`. External sources may document provenance. See [ADR 0001](../docs/adr/0001-repository-first-coordination.md).

## Start here

- [Onboarding](../docs/onboarding.md) walks through the architecture, the checks and picking up a task.
- The [documentation index](../docs/index.md) routes each area to its guide, package and tests.
- Work items use the [work-item issue template](../.github/ISSUE_TEMPLATE/work-item.yml) with a `[KEY]` title; labels and statuses follow [GitHub governance](../docs/github-governance.md).
- Decisions between alternatives go through an [ADR](../docs/adr/README.md); the merged ADR decides.

## File map

| File | Contains |
| --- | --- |
| [`README.md`](README.md) | This coordination model and the file map. |
| [`../docs/onboarding.md`](../docs/onboarding.md) | Canonical onboarding and the check commands. |
| [`../docs/index.md`](../docs/index.md) | Area → guide → package → tests routing. |
| [`../docs/github-governance.md`](../docs/github-governance.md) | Issue/PR forms, label taxonomy, merge rules. |
| [`../docs/adr/`](../docs/adr/README.md) | Decision records and the ADR process. |
| [`../CONTRIBUTING.md`](../CONTRIBUTING.md) | Before-PR checklist and handoff rules. |
| [`../.github/pull_request_template.md`](../.github/pull_request_template.md) | PR structure: scope, contracts, verification, handoff. |
