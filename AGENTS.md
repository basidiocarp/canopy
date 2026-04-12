# Canopy Agent Notes

## Purpose

Canopy owns local-first coordination state for multi-agent work. Work here should keep the store, coordination tools, and transport surfaces separate. Canopy tracks ownership, handoffs, and evidence; it should not absorb memory, lifecycle capture, or agent execution.

---

## Source of Truth

- `src/store/`: SQLite schema, row mapping, and persistence traits.
- `src/tools/`: transport-agnostic coordination logic.
- `src/api.rs`: snapshot and task-detail read models.
- `src/mcp/`: MCP tool surface and schema.
- `../septa/`: authoritative schemas for snapshot, task-detail, evidence, and handoff contracts.

If a public Canopy payload changes, update the matching `../septa/` schema and fixture first.

---

## Before You Start

Before writing code, verify:

1. **Owning layer**: decide whether the change belongs in store, tools, API read models, or MCP wiring.
2. **Contracts**: if task, handoff, evidence, or snapshot payloads change, read the matching `../septa/` files first.
3. **Ownership model**: keep assignment, claiming, and handoff explicit.
4. **Validation target**: decide whether the change needs store tests, tool tests, MCP checks, or all of them.

---

## Preferred Commands

Use these for most work:

```bash
cargo build --release
cargo test
```

For targeted work:

```bash
cargo test task
cargo test handoff
cargo clippy
cargo fmt --check
```

---

## Repo Architecture

Canopy is healthiest when coordination state, transport, and read models stay distinct.

Key boundaries:

- `src/store/`: SQLite ownership and migrations.
- `src/tools/`: coordination semantics shared by CLI and MCP.
- `src/mcp/`: stdio transport and tool exposure.
- `src/api.rs`: operator-facing read models.

Current direction:

- Keep coordination explicit instead of inferred from chat or runtime heuristics.
- Keep evidence as references rather than copied blobs.
- Keep CLI and MCP logic aligned through shared tool implementations.

---

## Working Rules

- Do not let MCP-specific concerns leak into the core coordination logic.
- Do not store copied sibling payloads when a typed reference is the intended model.
- Treat enum or payload changes as contract work and update `../septa/` in the same change.
- Back lifecycle and ownership changes with tests close to the store or tool layer that owns them.
- Validate septa contracts after changing any cross-project payload: `cd septa && bash validate-all.sh`

---

## Multi-Agent Patterns

For substantial Canopy work, default to two agents:

**1. Primary implementation worker**
- Owns the touched store, tool, API, or MCP layer
- Keeps the write scope inside Canopy unless a real contract update requires `../septa/`

**2. Independent validator**
- Reviews the broader shape instead of redoing the implementation
- Specifically looks for ownership-model drift, missing contract updates, store-vs-transport confusion, and evidence-shape regressions

Add a docs worker when `README.md`, `CLAUDE.md`, `AGENTS.md`, or public docs changed materially.

---

## Skills to Load

Use these for most work in this repo:

- `basidiocarp-rust-repos`: repo-local Rust workflow and validation habits
- `systematic-debugging`: before fixing unexplained coordination failures
- `writing-voice`: when touching README or docs prose

Use these when the task needs them:

- `test-writing`: when coordination behavior changes need stronger coverage
- `basidiocarp-workspace-router`: when the change may spill into `septa` or another repo
- `tool-preferences`: when exploration should stay tight

---

## Done Means

A task is not complete until:

- [ ] The change is in the right store, tool, API, or MCP layer
- [ ] The narrowest relevant validation has run, when practical
- [ ] Related schemas, fixtures, or docs are updated if they should move together
- [ ] Any skipped validation or follow-up work is stated clearly in the final response

If validation was skipped, say so clearly and explain why.
