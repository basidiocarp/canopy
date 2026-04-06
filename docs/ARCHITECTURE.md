# Canopy Architecture

Canopy is a single-crate Rust binary with CLI, MCP, and SQLite store layers.
Its core job is to make multi-agent coordination explicit: ownership,
handoffs, evidence, and operator attention live in a ledger instead of being
reconstructed from chat history. This document covers the system boundary,
request flow, and storage model.

---

## Design Principles

- **Task-first, not chat-first** — durable work state is stored as tasks,
  assignments, handoffs, and events, not inferred from free-form threads.
- **Evidence before conclusion** — decisions attach to `EvidenceRef` rows so
  operator views can explain why a task moved or closed.
- **One owner at a time** — tasks may have many participants, but the active
  owner stays explicit in the ledger.
- **Read models over raw tables** — operator surfaces consume snapshots and
  summaries, not ad hoc SQL across event history.
- **Local-first orchestration** — the first release assumes one machine, a
  local SQLite database, and sibling tools reached by reference.

---

## System Boundary

### Canopy owns

- Agent registration and heartbeat state
- Task lifecycle, assignment history, and review state
- Structured handoffs between agents and operators
- Task-scoped Council threads
- Evidence references to sibling tools
- Read models for operator attention and queue views

### Hyphae owns

- Memory, recall logging, and outcome signals
- Session history and long-lived project context
- Retrieval and ranking across stored memories

### Cortina owns

- Host adapter lifecycle capture
- Structured runtime events and validation signals
- Session bridge metadata

### Cap owns

- Operator-facing dashboards, timelines, and drilldown views
- Repair and intervention entry points

### Stipe owns

- Installation, setup, repair, and host registration

Canopy should reference Hyphae, Cortina, Mycelium, and Rhizome as evidence
sources. It should not absorb their storage or become the system of record for
installation, memory, or host repair.

---

## Workspace Structure

```text
src/
├── main.rs          # CLI entry point, DB path resolution, command routing
├── cli.rs           # Clap command surface
├── models.rs        # Ledger entities, enums, read-model structs
├── api.rs           # Snapshot and API-facing projection helpers
├── mcp/             # MCP protocol, schema, and server wiring
├── store/           # SQLite schema, migrations, and table-specific mutations
└── tools/           # MCP tool handlers layered over the store
```

Canopy compiles into a single binary.

- **`main.rs`**: Opens the store, parses CLI input, and routes to CLI or MCP
  entry points.
- **`models.rs`**: Defines the contract for tasks, handoffs, evidence, and
  read-model summaries. This is the shape the rest of the tool protects.
- **`store/`**: Owns schema migration and all durable mutations. Business rules
  around ownership, timing, and cascades belong here.
- **`tools/`**: Exposes the ledger over MCP without letting transport concerns
  leak into storage code.
- **`api.rs`**: Builds operator-friendly views such as snapshots and attention
  summaries from raw ledger data.

---

## Request Flow

When a CLI command or MCP tool call arrives:

1. **Open the ledger** (`main::main`)
   Parses CLI arguments, resolves the SQLite path, and opens `Store`.
   Example: `canopy task create ...` opens `.canopy/canopy.db` unless
   overridden.

2. **Route the command** (`main::run_command` or `mcp::server::run_server`)
   Chooses the task, handoff, evidence, API, or MCP path.
   Example: `Commands::Task` goes to `handle_task_command`; `serve` starts the
   MCP server.

3. **Apply domain rules** (`Store::*` in `store/tasks.rs`, `store/handoffs.rs`,
   `store/evidence.rs`)
   Validates allowed state transitions, ownership rules, due and expiry timing,
   and foreign-key references.
   Example: accepting a handoff updates the handoff row without silently
   changing task ownership.

4. **Record history** (`store/events.rs`, `store/assignments.rs`,
   `store/council.rs`)
   Writes task events, assignment history, Council messages, and heartbeat
   records so the ledger stays replayable.
   Example: a status change writes both the updated task row and a
   `task_events` entry.

5. **Project a response** (`api.rs`, `tools/*.rs`, JSON serialization)
   Returns the updated entity, a task detail projection, or an operator
   snapshot.
   Example: `canopy api snapshot --preset attention` returns a read model, not
   raw table dumps.

---

## Data Model

### Task

```rust
pub struct Task {
    pub task_id: String,
    pub title: String,
    pub project_root: String,
    pub owner_agent_id: Option<String>,   // at most one active owner
    pub status: TaskStatus,               // execution lifecycle
    pub verification_state: VerificationState,
    pub due_at: Option<String>,           // RFC3339 when present
    pub review_due_at: Option<String>,    // RFC3339 when present
    pub scope: Vec<String>,               // file paths or globs touched by work
}
```

### Handoff

```rust
pub struct Handoff {
    pub handoff_id: String,
    pub task_id: String,
    pub from_agent_id: String,
    pub to_agent_id: String,
    pub handoff_type: HandoffType,
    pub due_at: Option<String>,           // cannot be later than expires_at
    pub expires_at: Option<String>,
    pub status: HandoffStatus,
    pub resolved_at: Option<String>,
}
```

### TaskStatus

```rust
pub enum TaskStatus {
    Open,             // exists but not yet claimed
    Assigned,         // owner selected, execution not started
    InProgress,       // active implementation work
    Blocked,          // waiting on dependency or operator action
    ReviewRequired,   // ready for review or verification
    Completed,        // work finished, not yet closed
    Closed,           // terminal success state
    Cancelled,        // terminal abandoned state
}
```

### Schema

10 tables. Key invariants:

- Deleting a task cascades to assignments, handoffs, Council messages,
  evidence refs, task events, and relationships.
- Task relationships are unique per `(source_task_id, target_task_id, kind)`.
- Active file locks are unique per `(file_path, worktree_id)` while
  `released_at` is null.
- Handoffs keep timing explicit; expired handoffs are not reopened into
  non-expired states.

---

## Testing

```bash
cargo test
cargo build --release
```

| Category | Count | What's Tested |
|----------|-------|---------------|
| Store and model tests | 80+ | Task transitions, ownership rules, evidence linking, relationship rules, heartbeat behavior |
| CLI and integration | 20+ | Command routing, JSON output, API snapshots, MCP-adjacent flows |
| Edge cases | 10+ | Timing validation, duplicate relationships, file-lock conflicts, cascade behavior |

Fixtures are mostly synthetic rows assembled in Rust tests. The important thing
to preserve is behavioral coverage around ledger invariants, not a snapshot of
every JSON payload.

---

## Key Dependencies

- **`rusqlite`** — local ledger storage, migrations, and relational constraints
  that keep Canopy honest.
- **`clap`** — the CLI contract. Many operator workflows still enter through the
  terminal even when the same behavior exists over MCP.
- **`spore`** — shared ecosystem transport and tool primitives.
- **`time`** — timestamp parsing and formatting for due dates, expiry windows,
  and read-model freshness.
