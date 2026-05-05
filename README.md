# Canopy

Local-first coordination runtime for multi-agent work. Tracks task ownership,
handoffs, evidence, and operator attention so parallel agents do not drift into
unstructured chat.

Named after the forest canopy, the layer above the understory that organizes
what sits beneath it.

Part of the [Basidiocarp ecosystem](https://github.com/basidiocarp).

---

## The Problem

Once more than one agent is active, work usually falls back to ad hoc notes,
paste-heavy handoffs, and incomplete operator visibility. You can see outputs,
but not who owns what, what is blocked, what needs review, or which evidence
supports a decision.

## The Solution

Canopy gives the ecosystem a local orchestration ledger. It records active
agents, task ownership, status changes, handoffs, Council threads, and evidence
references in one place. That makes operator views possible without scraping
chat logs or reconstructing state from memory.

---

## The Ecosystem

| Tool | Purpose |
|------|---------|
| **[annulus](https://github.com/basidiocarp/annulus)** | Cross-ecosystem operator utilities |
| **[canopy](https://github.com/basidiocarp/canopy)** | Multi-agent coordination runtime |
| **[cap](https://github.com/basidiocarp/cap)** | Web dashboard for the ecosystem |
| **[cortina](https://github.com/basidiocarp/cortina)** | Lifecycle signal capture and session attribution |
| **[hyphae](https://github.com/basidiocarp/hyphae)** | Persistent agent memory |
| **[hymenium](https://github.com/basidiocarp/hymenium)** | Workflow orchestration engine |
| **[lamella](https://github.com/basidiocarp/lamella)** | Skills, hooks, and plugins for coding agents |
| **[mycelium](https://github.com/basidiocarp/mycelium)** | Token-optimized command output |
| **[rhizome](https://github.com/basidiocarp/rhizome)** | Code intelligence via tree-sitter and LSP |
| **[spore](https://github.com/basidiocarp/spore)** | Shared transport and editor primitives |
| **[stipe](https://github.com/basidiocarp/stipe)** | Ecosystem installer and manager |
| **[volva](https://github.com/basidiocarp/volva)** | Execution-host runtime layer |

> **Boundary:** `canopy` owns coordination state. `hyphae` owns memory,
> `cortina` owns lifecycle capture, `stipe` owns installation and host setup,
> and `cap` owns the operator UI. `canopy` keeps the coordination ledger and
> read models explicit; it does not absorb memory policy, presentation policy,
> or dashboard layout.

---

## Quick Start

```bash
# Build or install
cargo install --path .

# Register yourself as the orchestrator
canopy agent register \
  --agent-id orchestrator-1 \
  --host-id will \
  --host-type claude-code \
  --host-instance local-machine \
  --model opus \
  --project-root /home/user/myproject \
  --worktree-id main \
  --role orchestrator

# Create work
canopy task create \
  --title "Fix hyphae decay formula" \
  --requested-by orchestrator-1

# Set priority and severity
canopy task triage \
  --task-id <task_id> \
  --changed-by orchestrator-1 \
  --priority high

# Assign work
canopy task assign \
  --task-id <task_id> \
  --assigned-to implementer-1 \
  --assigned-by orchestrator-1

# Inspect what needs attention
canopy api snapshot --preset attention

# Quick agent and task snapshot
canopy situation --agent-id orchestrator-1
```

---

## How It Works

```text
Agents / operators         Canopy                      Ecosystem
──────────────────         ──────                      ─────────
register / heartbeat ─►    agent registry
create / assign task ─►    task ledger
post proposal / handoff ─► Council + handoff store ─► evidence refs
open snapshot / task ─►    read models               ─► cap
```

1. Register agents: record host identity, role, and heartbeat state.
2. Track tasks: persist creation, assignment, status, and closure events.
3. Attach evidence: link task decisions to Hyphae, Cortina, Rhizome, and Mycelium outputs.
4. Manage handoffs: move work between implementers, validators, and operators with typed status.
5. Serve operator views: expose task detail, timeline, and attention-focused snapshots.

Canopy is also the first consumer for Septa's `workflow-participant-runtime-identity-v1` contract. It links `workflow_id`, `participant_id`, `runtime_session_id`, `project_root`, and `worktree_id` into the task workflow context instead of treating execution-host identity as ad hoc metadata.

---

## Core Features

| Role | Model or Actor | What It Does |
|------|----------------|--------------|
| Orchestrator | Human or strongest model | Creates tasks, reviews evidence, records decisions |
| Implementer | Sonnet or Codex | Claims work, writes changes, submits handoffs |
| Validator | Haiku or reviewer | Verifies results and resolves review tasks |

---

## What Canopy Owns

- Agent registry and heartbeat history
- Task ledger and lifecycle state
- Structured handoff protocol
- Task-scoped Council threads
- External evidence references attached to work
- Operator read models derived from ledger state, not UI state
- Evidence refs that keep `source_kind`, `source_ref`, and cross-links explicit
- Workflow linkage that can consume shared identity contracts from `septa` instead of inventing repo-local runtime ids

## What Canopy Does Not Own

- Long-term memory or retrieval ranking: handled by `hyphae`
- Hook and session capture: handled by `cortina`
- Installation and host repair: handled by `stipe`
- General-purpose dashboards: handled by `cap`
- Skills, hooks, and packaging content: handled by `lamella`

---

## Key Features

- Local-first ledger: stores orchestration state under `.canopy/` instead of overloading another tool's database.
- Structured handoffs: supports due dates, expiration, typed statuses, and review-oriented transfer flows.
- Council threads: records proposals, objections, evidence, decisions, and handoffs per task.
- Attention views: exposes presets for blocked work, overdue tasks, critical items, and review queues.
- MCP surface: can be consumed through CLI or MCP tools instead of shell parsing.

---

## Architecture

```text
canopy (single binary)
├── src/store/   local ledger and persistence
├── src/mcp/     MCP server and schema wiring
├── src/tools/   task, handoff, council, and evidence handlers
├── src/         CLI entry point and models
└── tests/       integration coverage
```

```text
Agent Management
  canopy agent register       register an agent with identity and capabilities
  canopy agent heartbeat      send a liveness signal and update current task
  canopy agent history        view agent activity history
  canopy agent list           list all registered agents

Task Lifecycle
  canopy task create          create a new task
  canopy task assign          assign a task to an agent
  canopy task claim           claim a task as the current agent
  canopy task complete        mark task done (fails if children are open)
  canopy task status          update task status and verification state
  canopy task triage          set priority, severity, and owner notes
  canopy task action          multi-purpose task action (bundled changes)
  canopy task verify          run a verification script against a task
  canopy task list            list all tasks in tree or flat view
  canopy task list-view       list tasks with filtering and presets
  canopy task show            display detailed task information

Handoff Management
  canopy handoff create       request review or transfer work to another agent
  canopy handoff resolve      accept, reject, or defer a handoff
  canopy handoff action       perform operator actions on a handoff
  canopy handoff list         list handoffs for a task

Evidence & Verification
  canopy evidence add         link external evidence to a task
  canopy evidence list        list all evidence for a task
  canopy evidence verify      check evidence validity

Council Sessions (Decision Threads)
  canopy council summon       open a council session for task discussion
  canopy council post         post a message to a council session
  canopy council show         display council messages for a task
  canopy council open         open a new council session
  canopy council close        close a council session with optional outcome
  canopy council status       list open sessions for a task
  canopy council join         add an agent to a council session

Workflow & Outcomes
  canopy outcome record       store a workflow-outcome-v1 JSON result
  canopy outcome list         list all recorded outcomes
  canopy outcome show         display outcome by workflow ID
  canopy outcome summary      print outcome counts by template and failure type

Dispatch & Policy
  canopy dispatch submit      create a task from a dispatch-request-v1 payload
  canopy policy show          display active MCP dispatch policy

File Coordination
  canopy files lock           lock files for exclusive agent access
  canopy files unlock         release locks for a task
  canopy files check          check for lock conflicts on files
  canopy files list           list all locked files

Notifications
  canopy notification list    list unread notifications (or all with --all)
  canopy notification mark-read mark a single notification as read
  canopy notification mark-all-read mark all notifications as read

Queues & Coordination
  canopy work-queue           show priority queue for an agent
  canopy import-handoff       import a handoff from a Markdown or JSON file
  canopy situation            view agent and task snapshot at a glance

Server & Snapshots
  canopy api snapshot         render operator views with filtering
  canopy serve                expose MCP tools for Claude Code
```

---

## Documentation

- [docs/README.md](docs/README.md): repo-local docs index
- [docs/architecture.md](docs/architecture.md): storage model, APIs, and design decisions
- [docs/mvp.md](docs/mvp.md): first release scope and behavior
- [ROADMAP.md](ROADMAP.md): planned coordination features and follow-up work

## Development

```bash
cargo build --release
cargo nextest run
cargo test
cargo clippy
cargo fmt
```

- Prefer `cargo nextest run` for the normal test loop.
- Keep `criterion` out of scope here until a concrete hot path is named.
- Use whole-command timing when a real operator path feels slow, for example
  `time cargo run -- api snapshot --preset attention`.

## Logging

Canopy writes diagnostic logs to stderr through Spore's shared logger so CLI
stdout and MCP stdio responses stay clean.

- Use `CANOPY_LOG` for repo-specific logging, for example
  `CANOPY_LOG=canopy=debug canopy serve --agent-id orchestrator`.
- `RUST_LOG` still works as the broader Rust fallback, but `CANOPY_LOG` is the
  intended operator knob for this binary.
- Logging is separate from normal product output: CLI JSON and MCP responses
  still flow on stdout, while operator diagnostics and tracing stay on stderr.
- Lifecycle span events are enabled by default so the shared span boundaries
  emit under normal operator runs instead of only appearing at elevated log
  levels.
- Most runtime diagnostics now flow through `CANOPY_LOG`, but a few CLI
  compatibility messages still write directly to stderr when they are part of
  the user-facing command surface.

## License

See repository license.
