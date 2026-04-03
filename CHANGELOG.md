# Changelog

## [0.3.1] - 2026-04-03

### Added

- **File conflict detection**: Scope-aware file lock conflict detection for multi-agent coordination
- **Orchestrator completion verification**: Completeness checker tool for verifying task completion before handoff
- **Handoff completeness checker**: Validates structured handoff payloads meet requirements
- **MCP schema updates**: Extended tool definitions for new conflict and completeness tools

### Changed

- Updated store traits, models, and schema for scope and conflict tracking
- Updated API layer, CLI, and MCP server for new tool surface
- Updated tests for contract alignment, schema drift, and store round-trips
- Bumped lockfile dependencies and version to 0.3.1

## [0.2.0] - 2026-03-31

### Added

- Evidence verification: Added best-effort evidence verification so Canopy can report whether stored evidence references are verified, stale, or unsupported.

### Changed

- Versioned evidence references: Stored evidence references now persist and emit `schema_version: "1.0"` so downstream consumers can validate the contract explicitly.
- Shared foundation paths: Default database path resolution now goes through Spore with a one-time migration bridge from the older local Canopy path.

### Fixed

- Cross-tool evidence round-trip: Cap-facing Canopy snapshots and task detail reads now use the same evidence-ref contract that Canopy persists internally.
