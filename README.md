# Canopy

`Canopy` is the coordination runtime for the Basidiocarp ecosystem.

Its orchestration model is called `Council`.

The first release is local-first:

- one machine
- multiple host adapters or instances
- multiple worktrees

Remote multi-machine coordination is a later phase.

`Canopy` is not a generic agent chat room. It is a task-scoped runtime for:

- registering active agents and the host references they run on
- assigning and handing off work
- attaching evidence to task decisions
- tracking who owns a task, what is blocked, and what needs review
- giving operators a clear view of active multi-agent work

## What `Canopy` Owns

- agent registry
- task ledger
- structured handoff protocol
- task-scoped `Council` threads
- external evidence references attached to task decisions

## What `Canopy` Does Not Own

- long-term memory and retrieval ranking
  - handled by `hyphae`
- host lifecycle capture
  - handled by `cortina`
- installation and repair flows
  - handled by `stipe`
- generic dashboard concerns
  - handled by `cap`
- host registration itself
  - referenced from `stipe` and `cortina`, not recreated inside `Canopy`

## MVP Direction

The first `Canopy` release should start small:

- local orchestration store
- agent registration with:
  - `host_id`
  - `host_type`
  - `host_instance`
  - heartbeat and status
- durable task creation, assignment, handoff, and closure
- timestamped task and handoff records for operator aging/sorting
- task lifecycle mutation with:
  - verification state
  - blocked reason
  - closure metadata
  - triage metadata:
    - priority
    - severity
    - acknowledgment state
    - operator note
  - persisted task-event history for creation, assignment, transfer, and status changes
- heartbeat history, not just latest heartbeat state
- handoff timing metadata:
  - `due_at`
  - `expires_at`
  - write-time timestamp validation
- typed protocol values for:
  - agent status
  - task status
  - handoff type and status
  - council message type
- task-scoped `Council` messages:
  - `proposal`
  - `objection`
  - `evidence`
  - `decision`
  - `handoff`
- explicit links to external evidence such as:
  - `hyphae` session ids
  - `cortina` event ids
  - `rhizome` impact-analysis results
  - `mycelium` explain or economics output
- an explicit read surface for operator tools via `Canopy` commands instead of direct database access
- explicit filter/sort-aware read models for snapshot consumers
- task detail read models that include lifecycle timeline rows and related heartbeat history, not just latest task state
- runtime attention semantics for operators:
  - task, handoff, and agent attention levels
  - heartbeat freshness summaries
  - server-side `attention` task view
  - explicit snapshot attention counts instead of dashboard-side heuristics
  - first-class snapshot presets and server-side triage filters
- runtime ownership and triage summaries:
  - assignment history
  - reassignment counts and latest assignment metadata
  - operator action hints for acknowledgment, review, reassignment, and aging/expired handoffs
- stronger agent/task linkage invariants for registration and heartbeat updates

## Storage Boundary

The MVP should give `Canopy` its own local orchestration store instead of trying to overload `hyphae` session state.

Recommended first path:

- state directory: `.canopy/`
- ledger database: `.canopy/canopy.db`
- artifact references stored as ids or URIs pointing into other ecosystem tools

This keeps orchestration state explicit and avoids forcing `hyphae` to become a task scheduler.

`cap` should consume `Canopy` through an explicit API or CLI surface, not by reading `canopy.db` directly.

## Usage

### Quick Start

```bash
# Install
cargo install --path .

# Register yourself as the orchestrator
canopy agent register --host-id will --host-type claude-code --role orchestrator

# Create a task
canopy task create --title "Fix hyphae decay formula" --priority high

# Assign it
canopy task assign <task_id> --agent-id will

# Update status as you work
canopy task status <task_id> --status in_progress
canopy task status <task_id> --status completed

# See what needs attention
canopy api snapshot --preset attention
```

### Agent Registration

Every agent (human or AI) registers with a host type and optional role:

```bash
# Human operator
canopy agent register --host-id will --host-type claude-code --role orchestrator

# Claude Code implementer
canopy agent register --host-id claude-impl --host-type claude-code --role implementer

# Codex implementer
canopy agent register --host-id codex-rust --host-type codex --role implementer

# Haiku validator
canopy agent register --host-id haiku-check --host-type claude-code --role validator
```

Agents send heartbeats to signal they're alive:

```bash
canopy agent heartbeat --agent-id claude-impl --status in_progress
```

### Task Lifecycle

```bash
# Create
canopy task create --title "Implement statusline" --priority high

# Assign to an agent
canopy task assign <task_id> --agent-id codex-rust

# Status updates
canopy task status <task_id> --status in_progress
canopy task status <task_id> --status blocked --reason "needs HttpClient in middleware layer"
canopy task status <task_id> --status completed

# View task detail with full timeline
canopy api task --task-id <task_id>
```

### Handoffs

Transfer work between agents with structured context:

```bash
# Agent finishes and hands off for review
canopy handoff create \
  --task-id <task_id> \
  --from-agent-id codex-rust \
  --to-agent-id haiku-check \
  --type request-review \
  --summary "Implemented statusline. All tests pass. See diff."

# Reviewer resolves the handoff
canopy handoff resolve <handoff_id> --status completed

# Or rejects it back
canopy handoff resolve <handoff_id> --status rejected
```

### Council Decisions

Structured deliberation on tasks:

```bash
# Propose an approach
canopy council post --task-id <task_id> --type proposal \
  --content "Use hardcoded pricing table, upgrade to LiteLLM later"

# Object with evidence
canopy council post --task-id <task_id> --type objection \
  --content "Hardcoded rates go stale. Use ccusage's LiteLLM approach."

# Record the decision
canopy council post --task-id <task_id> --type decision \
  --content "Start with hardcoded table. Revisit if rates change within 3 months."

# View the thread
canopy council show --task-id <task_id>
```

### Evidence

Attach references to external ecosystem data:

```bash
canopy evidence add \
  --task-id <task_id> \
  --source-kind hyphae_session \
  --source-ref "01JNQSESS000000000000000" \
  --label "Session where decay bug was discovered"

# Verify evidence is still valid
canopy evidence verify --task-id <task_id>
```

### Operator Views

```bash
# Full snapshot with attention scoring
canopy api snapshot

# Filtered views
canopy api snapshot --preset attention       # what needs your attention now
canopy api snapshot --preset critical        # critical priority only
canopy api snapshot --preset blocked         # blocked tasks
canopy api snapshot --preset overdue         # past deadline
canopy api snapshot --preset review-queue    # waiting for review

# Task detail with timeline and evidence
canopy api task --task-id <task_id>
```

## Multi-Model Orchestration

Canopy supports a hierarchy where different AI models play different roles:

| Role | Model | What It Does |
|------|-------|-------------|
| **Orchestrator** | Opus / Human | Creates tasks, assigns work, reviews evidence, makes decisions |
| **Implementer** | Sonnet / Codex | Claims tasks, writes code, submits handoffs with summaries |
| **Validator** | Haiku | Claims verification tasks, runs checks, reports pass/fail |

### Example Flow

```bash
# Orchestrator creates the work breakdown
canopy task create --title "Implement feature X" --priority high
canopy task create --title "Write implementation" --parent <parent_id> \
  --required-role implementer
canopy task create --title "Verify compilation" --parent <parent_id> \
  --required-role validator
canopy task create --title "Review spec" --parent <parent_id> \
  --required-role validator

# Implementer claims and does the work
canopy task assign <impl_id> --agent-id sonnet1
canopy task status <impl_id> --status in_progress
# ... does the work ...
canopy task status <impl_id> --status completed
canopy handoff create --task-id <impl_id> --from-agent-id sonnet1 \
  --type request-review --summary "Done. See diff."

# Validator claims and checks
canopy task assign <verify_id> --agent-id haiku1
# ... runs cargo test, cargo clippy ...
canopy task status <verify_id> --status completed

# Orchestrator sees all children complete on the parent task
canopy api task --task-id <parent_id>
# → shows AllChildrenComplete attention reason
```

### Usage Patterns

Pattern 1: Operator-driven (start here)

You create tasks, paste handoffs into agents, verify results, update canopy.
Full control, you're the orchestrator.

Pattern 2: Agent-driven

Agents call canopy CLI themselves. Add to each agent's CLAUDE.md:

```markdown
When assigned a canopy task, update your status:
- On start: `canopy task status <id> --status in_progress`
- On complete: `canopy task status <id> --status completed`
- On blocked: `canopy task status <id> --status blocked --reason "..."`
```

Pattern 3: MCP server

Canopy exposes 30 tools via MCP (`canopy serve`), like hyphae and rhizome.
Agents call `canopy_create_task`, `canopy_claim_task`, `canopy_update_status`
as structured tool calls instead of CLI shell-outs. Lower token cost,
structured responses, no shell parsing. The MCP server is registered in
Claude Code's MCP config after `stipe init`.

## Docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)—design decisions and data model
- [docs/MVP.md](docs/MVP.md)—first release specification
- [ROADMAP.md](ROADMAP.md)—project-level roadmap
