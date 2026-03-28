# Canopy MVP

This page defines the first practical `Canopy` release.

The current codebase only implements the foundation layer of this MVP:

- local SQLite ledger
- agent registry
- latest heartbeat tracking
- heartbeat history
- tasks
  - verification state
  - blocked reason
  - closure metadata
  - task-event history rows for creation, assignment, transfer, and status changes
  - created/updated timestamps for operator sorting and aging
- handoffs
  - created/updated/resolved timestamps
- council messages
- evidence refs
- evidence navigation fields so downstream operator tools do not have to infer every link from `source_kind`
- explicit read models through `api snapshot` and `api task`
- snapshot filters and sorting for server-side saved-view support
- runtime attention summaries for:
  - tasks
  - handoffs
  - agents
- server-side `attention` view and aggregate attention counts for operator consumers

## Goal

Make multi-agent work traceable and transferable across local host adapters or instances without turning the ecosystem into a bot chat system.

## MVP Scope

### 1. Agent registry

Track live workers with:

- host id
- host type
- host instance
- model
- project root
- worktree
- current task
- status
- heartbeat

### 2. Task ledger

Allow tasks to be:

- created
- assigned
- accepted
- blocked
- handed off
- reviewed
- closed

### 3. Structured handoff protocol

The first handoff messages should cover:

- help
- review
- ownership transfer
- verification request
- decision recording
- closure

### 4. Task-scoped `Council` thread

Each task gets a thread of typed messages:

- `proposal`
- `objection`
- `evidence`
- `decision`
- `handoff`
- `status`

### 5. Evidence model

Each task decision or handoff can attach evidence refs from:

- `hyphae`
- `cortina`
- `mycelium`
- `rhizome`
- manual operator notes

## Initial CLI Surface

Recommended first commands:

```text
canopy agent register
canopy agent list
canopy agent heartbeat
canopy agent history
canopy task create
canopy task assign
canopy task status
canopy task list
canopy task list-view
canopy task show
canopy handoff create
canopy handoff resolve
canopy evidence add
canopy evidence list
canopy council post
canopy council show
canopy api snapshot
canopy api task
```

## Initial API Surface

Recommended first endpoints:

- `POST /agents/register`
- `POST /agents/{id}/heartbeat`
- `GET /agents`
- `GET /agents/heartbeats`
- `POST /tasks`
- `POST /tasks/{id}/assign`
- `POST /tasks/{id}/status`
- `GET /tasks`
- `GET /tasks/{id}`
- `POST /handoffs`
- `POST /handoffs/{id}/resolve`
- `POST /tasks/{id}/messages`
- `GET /tasks/{id}/messages`

## First `Cap` Integration

`Cap` should not try to model orchestration itself. It should consume `Canopy` through an explicit API or CLI surface and render:

- active agents
- active tasks
- blocked tasks
- pending handoffs
- review-required tasks
- latest `Council` decisions
- explicit runtime attention and freshness state instead of inferring everything from timestamps

The first useful views are:

- active task board
- task detail drawer
- handoff queue
- per-agent activity panel
- saved views that can be backed by Canopy-side filters instead of purely client heuristics

Task detail should include:

- latest task state
- lifecycle event history
- handoffs
- council messages
- evidence refs

## First `Stipe` Integration

`Stipe` should not run `Council` logic. It should only ensure that:

- supported hosts are installed
- required adapters are configured
- `Canopy` can discover healthy local hosts

## Deferred From MVP

- fleet-wide autoscheduling
- arbitration or judge workflows
- automatic task claiming and deduplication
- remote multi-machine coordination
- generalized planning engine

## First Success Criteria

- two agents on different local host adapters or host instances can share one task with explicit ownership transfer
- an operator can see the handoff and evidence in one place
- a blocked task is visible without reading raw logs
- a completed task records who closed it, its verification state, and what evidence justified closure
- an operator can inspect the sequence of task lifecycle changes without inferring it from snapshots
