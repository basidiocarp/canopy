# Canopy Roadmap

This page is the Canopy-specific backlog. The workspace [ROADMAP.md](../ROADMAP.md) keeps the ecosystem sequencing, and [MASTER-ROADMAP.md](../MASTER-ROADMAP.md) keeps the cross-repo summary.

## Recently Shipped

- Initial repo and spec scaffold.
- Naming split:
  - `Canopy` as the repo and runtime
  - `Council` as the orchestration model
- First concrete architecture boundary for orchestration state, evidence refs, and integration ownership.
- First Rust crate with:
  - local SQLite ledger
  - agent registry
  - latest heartbeat tracking
  - heartbeat history
  - tasks
    - verification state
    - blocked reason
    - closure metadata
    - persisted task-event history
    - created/updated timestamps
  - handoffs
    - created/updated/resolved timestamps
  - council messages
  - evidence refs
    - typed navigation fields for sessions, memories, and code
  - typed protocol enums and validation
  - explicit snapshot and task-detail read models
  - snapshot filtering and sorting for saved-view support
  - first-class snapshot presets and server-side triage filters
  - runtime attention/freshness summaries for tasks, handoffs, and agents
  - server-side `attention` view and aggregate attention counts
  - task triage metadata:
    - priority
    - severity
    - acknowledgment state
    - operator note
  - handoff due/expiry semantics with validation
  - assignment-history-backed ownership summaries
  - task and agent heartbeat summaries
  - operator action hints for task acknowledgment, review, reassignment, and handoff follow-up/expiry
  - stronger registration/heartbeat linkage invariants
  - CLI coverage and tests

## Next

### Ledger and registry foundation

Complete the first local orchestration store beyond the current foundation with:

- stronger typed protocol/state validation
- richer task timeline metadata beyond the current event rows
- richer task/operator action metadata beyond the current triage foundation
- operator mutations that consume the new action hints cleanly

### MVP CLI

Finish the first CLI surface with:

- richer task/detail queries on read surfaces
- more task and handoff mutation commands around operator workflows

### Read API transport

Keep the explicit read boundary but decide whether the next operator-facing transport should stay CLI-backed or add a local HTTP surface for `cap`.

### `Cap` operator integration

Expose active agents, active tasks, blocked tasks, pending handoffs, and council summaries to `cap`.

### Evidence integration

Attach `hyphae`, `cortina`, `mycelium`, and `rhizome` references as task evidence without duplicating their full payloads.

## Later

### Arbitration and review flows

Add judge, dispute, and multi-review workflows when agents disagree.

### Capability routing

Route work by host and model strengths instead of static host preference.

### Task claiming and de-duplication

Prevent duplicate work when multiple agents target the same project or queue.

### Fleet coordination

Expand from local coordination into broader host and worker fleets once the single-machine path is boring.

## Research

### Transport boundary

Decide whether the first runtime should be CLI-only, API-first, or dual-surface from the start.

### Heartbeat model

Decide whether heartbeats should be pushed by adapters, pulled by `Canopy`, or derived from event streams.

### Shared-state boundary

Keep deciding how much orchestration state belongs in `Canopy` versus references into `hyphae`.
