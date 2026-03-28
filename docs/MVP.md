# Canopy MVP

This page defines the first practical `Canopy` release.

The current codebase only implements the foundation layer of this MVP:

- local SQLite ledger
- agent registry
- latest heartbeat tracking
- tasks
- handoffs
- council messages
- evidence refs
- explicit read models through `api snapshot` and `api task`

Heartbeat history beyond the latest heartbeat, richer task status mutation, and HTTP transport are still next-slice work.

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
canopy task create
canopy task assign
canopy task list
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

The first useful views are:

- active task board
- task detail drawer
- handoff queue
- per-agent activity panel

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
- a completed task records who did the work and what evidence justified closure
