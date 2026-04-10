# Canopy Roadmap

This page is the Canopy-specific backlog. The workspace [ROADMAP.md](../docs/workspace/ROADMAP.md) keeps the ecosystem sequencing and cross-repo priorities.

## Recently Shipped

- Canopy now has a real local orchestration foundation instead of a repo stub. The first pass covers the ledger, agent registry, task and handoff records, council messages, evidence references, and the read models needed for operator views.
- The naming split is settled. `Canopy` is the runtime and repository, while `Council` names the orchestration model that sits inside it.
- Task state is no longer just a queue. The current schema tracks verification state, blocking reason, closure metadata, triage fields, due and expiry semantics, ownership history, heartbeat summaries, and operator action hints.
- The first CLI and test surface is in place. That gives the project a stable base for operator queries and workflow mutations instead of forcing everything through direct database inspection.

## Next

### Ledger and registry hardening

The next step is making the current store boring. Canopy needs stronger protocol validation, richer task timelines, and cleaner operator mutations so the read models and the write path stay aligned as the orchestration surface grows.

### Operator workflow commands

The CLI needs to move past basic scaffolding. The immediate priority is deeper task and handoff queries plus the mutation commands that let an operator acknowledge, reassign, review, and close work without touching internal tables.

### Read transport for Cap

Canopy already has an explicit read boundary. The open product question is whether the next operator surface stays CLI-backed for a while or graduates to a local HTTP layer that `cap` can consume directly.

### Evidence integration

Canopy should attach evidence from Hyphae, Cortina, Mycelium, and Rhizome without copying their payloads into its own store. This keeps the orchestration layer small while still giving tasks enough context to prove what happened.

### Cap operator integration

Canopy becomes useful when operators can see it. The near-term goal is to expose active agents, active tasks, blocked work, pending handoffs, and council summaries in `cap` without building a second orchestration model in the UI.

## Later

### Arbitration and review flows

Canopy will eventually need judge, dispute, and multi-review workflows. That work can wait until the basic task and handoff path is solid and evidence-backed.

### Capability routing

Task assignment should eventually follow host and model strengths instead of a fixed preference order. The right model depends on whether the job is orchestration, implementation, or narrow validation.

### Task claiming and de-duplication

Canopy should prevent two agents from quietly doing the same work. That becomes more important once the local single-operator path is stable and parallel task claims become common.

### Fleet coordination

The long-term direction is broader host and worker coordination. It should not move ahead of the single-machine path, because the local operator loop still needs to become routine first.

## Research

### Transport boundary

The first runtime could stay CLI-first, become API-first, or support both from the start. The right answer depends on how quickly `cap` needs direct read access and whether local automation also wants a stable API.

### Heartbeat model

Canopy still needs to decide whether heartbeats are pushed by adapters, pulled by the runtime, or inferred from event streams. That choice affects both correctness and how much runtime state the system has to own.

### Shared-state boundary

The open architectural question is how much state belongs inside Canopy versus how much should remain references into Hyphae and the rest of the ecosystem. The answer needs to keep orchestration useful without duplicating every lower-level store.
