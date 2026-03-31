# Changelog

## [0.2.0] - 2026-03-31

### Added

- **Evidence verification**: Added best-effort evidence verification so Canopy can report whether stored evidence references are verified, stale, or unsupported.

### Changed

- **Versioned evidence references**: Stored evidence references now persist and emit `schema_version: "1.0"` so downstream consumers can validate the contract explicitly.
- **Shared foundation paths**: Default database path resolution now goes through Spore with a one-time migration bridge from the older local Canopy path.

### Fixed

- **Cross-tool evidence round-trip**: Cap-facing Canopy snapshots and task detail reads now use the same evidence-ref contract that Canopy persists internally.
