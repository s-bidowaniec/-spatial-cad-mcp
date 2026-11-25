# Architecture Decision Records

Start from [planning](../../planning/README.md) and [GitHub governance](../github-governance.md). The index separates accepted decisions from proposals awaiting review.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-repository-first-coordination.md) | Repository First Coordination | Accepted |

## Decision flow

1. In an Issue, point at the problem, the contracts, the consumers and the feasibility boundaries. If a decision between alternatives is needed, open a PR with a separate numbered ADR in status `Proposed`. Use the [ADR template](template.md).
2. Record context, alternatives, decision, consequences and verification. The contract owner and a representative of each changed consumer check consistency. For license and data decisions, project-owner acceptance complements the technical opinion.
3. On acceptance set the status to `Accepted`, update the index and, where acceptance changes, the docs it affects. Record a change to an earlier decision under a new number and link the previous one, marked `Superseded`. Link research and discussions as justification. The merged ADR decides.
