# ADR 0001. Repository First Coordination

**Status** Accepted

**Context** Planning and evidence risked living in external sheets and chats, so picking up a task could require context from outside `main`.

**Decision** GitHub Issues are the authoritative work items. Versioned documents in this repo describe decisions, dependencies, claims and evidence: [planning/](../../planning/README.md), [docs/](../index.md), [ADRs](README.md). A merged ADR settles an architecture decision. Information needed to continue the work lands in the repository before the task is handed off.

**Alternatives** An external board requires synchronization and the same permissions for every agent. Issues alone suffice as a task register, but they make joint review of architecture, dependencies and plans harder within one PR.

**Consequences and verification** The reviewer checks consistency of the Issue, the status and the links. External material may serve as a source. Plan history is reviewable in Git.
