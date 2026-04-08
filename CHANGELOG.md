# Changelog

All notable changes to Canopy are documented in this file.

## [Unreleased]

### Changed

- **Changelog format**: Release headings and entry structure now follow the
  shared ecosystem changelog template.

## [0.5.7] - 2026-04-08

### Changed

- **Shared logging rollout**: Canopy now initializes logging through Spore's
  app-aware `CANOPY_LOG` path instead of relying on generic runtime setup.
- **Workflow tracing**: CLI command dispatch, MCP request handling, Cortina
  audit subprocesses, and Hyphae session verification now emit shared tracing
  spans with workspace-aware context for faster failure localization.

### Fixed

- **Operator guidance**: Docs now distinguish debug logging from normal CLI and
  MCP stdout output.

## [0.3.1] - 2026-04-03

### Added

- **File conflict detection**: Canopy now tracks scope-aware file lock
  conflicts so multi-agent work can surface overlapping edits before they
  collide.
- **Completion verification**: Added completeness-check tooling for task
  handoffs and orchestrator review.
- **Expanded MCP schema**: Tool definitions now cover the conflict and
  completeness surfaces introduced in this release.

### Changed

- **Conflict-aware ledger model**: Store traits, models, and schema now carry
  scope and conflict-tracking fields through the main coordination path.
- **Operator surfaces**: The API layer, CLI, and MCP server now expose the new
  conflict and verification flows consistently.

## [0.2.0] - 2026-03-31

### Added

- **Evidence verification**: Canopy now reports whether stored evidence
  references are verified, stale, or unsupported.

### Changed

- **Versioned evidence references**: Evidence rows now persist and emit
  `schema_version: "1.0"` so downstream consumers can validate the contract.
- **Shared foundation paths**: Default database resolution now flows through
  Spore, with a one-time migration bridge from the older local Canopy path.

### Fixed

- **Evidence roundtrip**: Cap-facing snapshots and task detail reads now use the
  same evidence-reference contract that Canopy persists internally.
