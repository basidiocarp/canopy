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
| **[canopy](https://github.com/basidiocarp/canopy)** | Multi-agent coordination runtime |
| **[cap](https://github.com/basidiocarp/cap)** | Web dashboard for the ecosystem |
| **[cortina](https://github.com/basidiocarp/cortina)** | Lifecycle signal capture and session attribution |
| **[hyphae](https://github.com/basidiocarp/hyphae)** | Persistent agent memory |
| **[lamella](https://github.com/basidiocarp/lamella)** | Skills, hooks, and plugins for coding agents |
| **[mycelium](https://github.com/basidiocarp/mycelium)** | Token-optimized command output |
| **[rhizome](https://github.com/basidiocarp/rhizome)** | Code intelligence via tree-sitter and LSP |
| **[spore](https://github.com/basidiocarp/spore)** | Shared transport and editor primitives |
| **[stipe](https://github.com/basidiocarp/stipe)** | Ecosystem installer and manager |
| **[volva](https://github.com/basidiocarp/volva)** | Execution-host runtime layer |

> **Boundary:** `canopy` owns coordination state. `hyphae` owns memory,
> `cortina` owns lifecycle capture, `stipe` owns installation and host setup,
> and `cap` owns the operator UI.

---

## Quick Start

```bash
# Build or install
cargo install --path .

# Register yourself as the orchestrator
canopy agent register --host-id will --host-type claude-code --role orchestrator

# Create and assign work
canopy task create --title "Fix hyphae decay formula" --priority high
canopy task assign <task_id> --agent-id will

# Inspect what needs attention
canopy api snapshot --preset attention
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

## What Canopy Does Not Own

- Long-term memory or retrieval ranking: handled by `hyphae`
- Hook and session capture: handled by `cortina`
- Installation and host repair: handled by `stipe`
- General-purpose dashboards: handled by `cap`

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
canopy agent register ...   register an agent identity
canopy task create ...      create work
canopy task assign ...      assign ownership
canopy handoff create ...   request review or transfer work
canopy api snapshot ...     render operator views
canopy serve                expose MCP tools
```

---

## Documentation

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): storage model, APIs, and design decisions
- [docs/MVP.md](docs/MVP.md): first release scope and behavior
- [ROADMAP.md](ROADMAP.md): planned coordination features and follow-up work

## Development

```bash
cargo build --release
cargo test
cargo clippy
cargo fmt
```

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
