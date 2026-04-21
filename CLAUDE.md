# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Canopy is a local-first coordination runtime for multi-agent work. It is a single Rust binary with a CLI, an MCP server, and a 31-tool MCP surface backed by SQLite for tasks, handoffs, evidence, and operator views. That surface includes `canopy_check_handoff_completeness` for handoff validation. Canopy owns coordination state; it does not own long-term memory, lifecycle capture, or installation.

---

## Operating Model

- Do not execute work on behalf of agents. Canopy tracks ownership and status, but the agents run elsewhere.
- Do not store copied external payloads. Evidence is a typed reference, not a duplicated blob.
- Do not require network connectivity. The runtime is local-first and single-machine by default.
- Do not make assignment implicit. Claiming, assignment, and handoff stay explicit.
- Do not treat Canopy as a memory system. It stores coordination state, not knowledge.

---

## What Canopy Does NOT Do

- Does not orchestrate workflows or manage dispatch decisions (Hymenium owns that)
- Does not retry or recover failed workflows (Hymenium owns that)
- Does not route work between agents based on capability (Hymenium owns that)

## Coordination Gating vs. Dispatch (Boundary Clarification)

Three areas in canopy look like dispatch but are coordination state:

- **`pre_dispatch_check` / `DispatchDecision`** (`src/runtime.rs`): gates whether a task assignment can proceed based on audit results. This is access control on coordination records — it calls `cortina audit-handoff` and returns Proceed/FlagForReview. It does not route work, schedule execution, or retry failures. Decision: keep in canopy as coordination gating.

- **`DispatchPolicy::evaluate()`** (`src/tools/policy.rs`): checks tool annotations (read-only, destructive, idempotent) against a policy before allowing assignment. This is annotation-based access control on the coordination layer, not workflow routing. Decision: keep in canopy as coordination access control.

- **Orchestration state machine** (`src/store/helpers/orchestration.rs`): manages queue states (ready/claimed/active/blocked/review/closed), worktree bindings, and review cycles as a SQLite-backed read model. This is coordination bookkeeping that Hymenium reads to make dispatch decisions — canopy tracks state, Hymenium acts on it. Decision: keep in canopy as coordination state.

---

## Failure Modes

- **Database locked**: waits on SQLite's busy timeout, then fails with a clear error instead of corrupting state.
- **Agent not registered**: returns an error that points back to the registration flow.
- **Task already claimed**: fails with the current owner so the caller can retry or reassign.
- **Evidence source unavailable**: verification reports `unsupported` or stale rather than inventing a result.
- **Missing parent records**: foreign-key checks fail fast with a clear error.

---

## State Locations

| What | Path |
|------|------|
| Ledger database | `~/.local/share/canopy/canopy.db` |
| Runtime logs | stderr |

---

## Build & Test Commands

```bash
cargo build --release
cargo install --path .

cargo test
cargo test task

cargo clippy
cargo fmt --check
cargo fmt
```

---

## Architecture

```text
src/
├── main.rs          CLI entry point and command dispatch
├── cli.rs           Clap command definitions
├── models.rs        Core task, handoff, agent, and evidence types
├── api.rs           Read models for snapshots and task detail
├── store/           SQLite schema, helpers, and persistence traits
├── tools/           Transport-agnostic coordination tools
└── mcp/             JSON-RPC server and tool schema
```

- **store/**: owns SQLite schema, migrations, row mapping, and the store trait.
- **tools/**: implements the transport-neutral task, queue, evidence, and handoff logic.
- **mcp/**: exposes the same coordination logic over stdio for Claude Code.

---

## Key Design Decisions

- **SQLite with explicit history**: local durability matters more than remote coordination right now.
- **References instead of copied payloads**: keeps Canopy narrow and prevents cross-tool data drift.
- **Transport-agnostic tools**: CLI and MCP use the same underlying coordination code.
- **Explicit ownership model**: assignment and handoff stay visible instead of being inferred from chat.

---

## Key Files

| File | Purpose |
|------|---------|
| `src/api.rs` | Builds snapshot and task-detail read models for operator views |
| `src/store/traits.rs` | Defines the `CanopyStore` trait that new persistence methods must satisfy |
| `src/tools/task.rs` | Owns task creation, update, completion, and blocking logic |
| `src/tools/handoff.rs` | Owns review and transfer workflow semantics |
| `src/mcp/schema.rs` | Defines the MCP tool surface exposed by `canopy serve` |

---

## Communication Contracts

### Outbound (this project sends)

| Contract | Target | Protocol | Schema |
|----------|--------|----------|--------|
| `evidence-ref-v1` | Cap and other readers | Local SQLite refs | `septa/evidence-ref-v1.schema.json` |
| `handoff-context-v1` | Receiving agent, Canopy, Cap | Handoff creation flow | `septa/handoff-context-v1.schema.json` |
| `canopy-snapshot-v1` | Cap | CLI `canopy api snapshot` | `septa/canopy-snapshot-v1.schema.json` |
| `canopy-task-detail-v1` | Cap | CLI `canopy api task --task-id <id>` | `septa/canopy-task-detail-v1.schema.json` |

**Source files:**
- `src/models.rs`: evidence and handoff types
- `src/api.rs`: snapshot and task-detail read models
- `src/store/`: persistence for those records

Breaking change impact: Cap and any other reader will misparse task, handoff, or evidence data.

### Inbound (this project receives)

Canopy does not accept pushed runtime payloads from sibling tools. It only stores its own coordination records and external references.

### Shared Dependencies

- **spore**: check `../ecosystem-versions.toml` before upgrading.
- **SQLite**: schema changes affect both CLI and MCP callers.

### Contract Validation

When changing output shapes that cross a project boundary, validate against septa:

```bash
cd ../septa && bash validate-all.sh
```

Check that this tool's schemas still pass before closing the change.

---

## Testing Strategy

- Unit tests focus on store behavior, task lifecycle rules, and MCP tool handling.
- Use the CLI and MCP surfaces interchangeably when testing a coordination change.
- Treat enum additions as contract changes; update schemas and fixtures in the same change.
- Test evidence, handoff, and snapshot changes against the contract files, not just local structs.
