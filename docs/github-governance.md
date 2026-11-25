# Issue and PR governance

Use the [GitHub issue forms](../.github/ISSUE_TEMPLATE/) to report a task, a change or a bug. The [PR template](../.github/pull_request_template.md) binds scope to evidence and handoff. Describe architecture decisions through an [ADR](adr/README.md).

| Group | Labels | Rule |
| --- | --- | --- |
| Type | `type:feature`, `type:bug`, `type:research`, `type:architecture` | One primary type. Decisions about a shared interface or acceptance evidence are architecture. |
| Priority | `priority:p0`, `priority:p1`, `priority:p2` | One. P0 is foundation, high risk or a release gate. P1 is planned scope. P2 is deferrable work. |
| Status | `status:needs-work`, `status:blocked`, `status:ready`, `status:in-progress`, `status:review` | One on an active issue. Ready requires the work-item fields complete (scope, dependencies, acceptance, evidence). Blocked must name the condition and its owner. Review points at a PR or evidence to assess. |
| Area | `area:ifc`, `area:dxf`, `area:gis`, `area:hdm`, `area:mesh`, `area:core`, `area:sandbox`, `area:ci`, `area:docs`, `area:planning` | At least one on the execution backlog. Add a second when the work crosses a module boundary (e.g. `area:core` + `area:mesh`). |

Triage assigns type, priority and area. The claim owner updates the status in the Issue. A status change replaces the previous label from the group. On closure, remove the active-status label and leave a link to the accepted evidence. Standard GitHub labels keep their historical meaning. If labels conflict with start conditions, check the Issue, the accepted evidence and the [onboarding](onboarding.md).

Merge through a PR after green CI and review by the appropriate reviewer. Changes to a shared contract (envelope, error codes, sandbox rules) are reviewed by the owner and representatives of the consumers. The [ADR process](adr/README.md) settles contested decisions.

[Onboarding](onboarding.md) · [CONTRIBUTING.md](../CONTRIBUTING.md) · [PR template](../.github/pull_request_template.md)
