# Canopy Architecture

`Canopy` is a task-scoped coordination runtime. `Council` is the interaction model inside it.

The first implementation target is local-first coordination across multiple host adapters or instances on one machine. Cross-machine fleet coordination is later work.

## Design Principles

- task-first, not chat-first
- evidence-first, not opinion-first
- explicit ownership and handoff
- durable state for operator visibility
- narrow boundaries with the rest of the ecosystem

## System Boundary

### `Canopy` owns

- agent registry
- host references attached to agents
- task ledger
- task assignment and ownership
- handoff protocol
- `Council` message model
- evidence references
- task replay and audit history

### `Hyphae` owns

- memory
- recall logging
- outcome signals
- session history
- retrieval and ranking

### `Cortina` owns

- host adapter lifecycle capture
- structured host events
- session bridge signals

### `Cap` owns

- operator-facing visibility
- dashboards, timelines, and drilldown
- repair entry points

### `Stipe` owns

- install, setup, repair, and host registration

`Canopy` should reference host identity from `stipe` and runtime health from `cortina`. It should not become the source of truth for installation or adapter repair.

## Core Entities

### AgentRegistration

Current foundation fields:

- `agent_id`
- `host_id`
- `host_type`
- `host_instance`
- `model`
- `project_root`
- `worktree_id`
- `status`
- `current_task_id`

Planned additions after the foundation slice:

- `capabilities`
- `heartbeat_at`
- `registered_at`
- `last_error`

Notes:

- `host_id` is the stable external reference for the host or adapter context the agent is running on.
- `host_instance` distinguishes multiple active hosts on one machine.
- `worktree_id` should be stable enough to separate parallel workers in one repo.

### Task

Current foundation fields:

- `task_id`
- `title`
- `description`
- `requested_by`
- `owner_agent_id`
- `project_root`
- `status`

Planned additions after the foundation slice:

- `priority`
- `verification_state`
- `created_at`
- `updated_at`
- `closed_at`

### TaskAssignment

Current foundation fields:

- `task_id`
- `assigned_to`
- `assigned_by`
- `reason`
- `assigned_at`

Planned additions after the foundation slice:

- `accepted_at`
- `released_at`

### Handoff

Current foundation fields:

- `handoff_id`
- `task_id`
- `from_agent_id`
- `to_agent_id`
- `handoff_type`
- `summary`
- `requested_action`
- `status`

Planned additions after the foundation slice:

- `created_at`
- `resolved_at`

### CouncilMessage

Current foundation fields:

- `message_id`
- `task_id`
- `author_agent_id`
- `message_type`
- `body`

Planned additions after the foundation slice:

- `related_handoff_id`
- `created_at`

Supported `message_type` values for the MVP:

- `proposal`
- `objection`
- `evidence`
- `decision`
- `handoff`
- `status`

### EvidenceRef

This is a planned next-slice entity. It is not in the first Rust foundation yet.

- `evidence_id`
- `task_id`
- `source_kind`
- `source_ref`
- `label`
- `summary`
- `created_at`

Expected `source_kind` values:

- `hyphae_session`
- `hyphae_recall`
- `hyphae_outcome`
- `cortina_event`
- `mycelium_command`
- `mycelium_explain`
- `rhizome_impact`
- `rhizome_export`
- `manual_note`

## Task State Model

Recommended MVP task states:

- `open`
- `assigned`
- `in_progress`
- `blocked`
- `review_required`
- `completed`
- `closed`
- `cancelled`

Ownership invariant for the first ledger:

- a task has at most one active owner at a time via `owner_agent_id`
- creating a handoff does not change ownership by itself
- ownership changes only when an explicit assignment or transfer is recorded
- blocked and review-required tasks still preserve their current owner until reassigned or closed

Recommended verification states:

- `unknown`
- `pending`
- `passed`
- `failed`

Recommended handoff statuses:

- `open`
- `accepted`
- `rejected`
- `expired`
- `cancelled`
- `completed`

## Handoff Types

The first protocol should support:

- `request_help`
- `request_review`
- `transfer_ownership`
- `request_verification`
- `record_decision`
- `close_task`

Each handoff should be explicit about:

- who owns the task before and after
- what evidence supports the request
- what outcome would count as completion

## Storage Model

The MVP should use a dedicated local SQLite store:

- `.canopy/canopy.db`

Rationale:

- orchestration data has different lifecycle and query patterns than `hyphae` memory
- task ownership should not be inferred from memory recall or host events
- local durability is enough for the first release

The store should contain:

- agent registrations
- tasks
- assignments
- handoffs
- council messages
- evidence refs
- heartbeat state

Likely next schema additions after the current foundation:

- heartbeat history beyond the latest heartbeat
- richer task verification and closure metadata

It should not duplicate:

- full memory payloads
- full event payloads from `cortina`
- long command outputs from `mycelium`

Instead, store references and small summaries.

## First Integration Points

### `Hyphae`

- attach `session_id`, `recall_event_id`, and `outcome_signal_id` to task evidence
- optionally create a task-scoped memory topic later, but do not require it for the MVP

### `Cortina`

- attach structured outcome and validation events as task evidence
- use host/session metadata to improve attribution, not to own tasks

### `Cap`

- show active agents
- show task ledger and task state
- show pending handoffs and blocked tasks
- show `Council` thread summaries per task
- consume `Canopy` through an API or CLI surface, not by reading `canopy.db` directly
- the current foundation exposes this boundary through task and snapshot read models

### `Stipe`

- register which hosts are available locally
- verify required adapters before an agent is considered healthy

## Non-Goals For The MVP

- free-form multi-agent forum behavior
- autonomous planning without explicit task ownership
- fleet autoscaling
- hidden reasoning capture
- chain-of-thought storage

## Open Questions

- whether agent heartbeats should be pushed by adapters or polled by `Canopy`
- whether `Canopy` should expose CLI only first, or CLI plus an HTTP API
- how the first API surface should authenticate or stay local-only before remote fleet work exists
