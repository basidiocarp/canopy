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
  - persisted task-event history for creation, assignment, transfer, and status changes
- heartbeat history, not just latest heartbeat state
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

## Storage Boundary

The MVP should give `Canopy` its own local orchestration store instead of trying to overload `hyphae` session state.

Recommended first path:

- state directory: `.canopy/`
- ledger database: `.canopy/canopy.db`
- artifact references stored as ids or URIs pointing into other ecosystem tools

This keeps orchestration state explicit and avoids forcing `hyphae` to become a task scheduler.

`cap` should consume `Canopy` through an explicit API or CLI surface, not by reading `canopy.db` directly.

## Docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- [docs/MVP.md](docs/MVP.md)
- [ROADMAP.md](ROADMAP.md)
