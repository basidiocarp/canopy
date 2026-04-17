//! Shared capability vocabulary for task routing.
//!
//! These string constants form the stable vocabulary both Canopy and Hymenium use when
//! expressing `required_capabilities` on tasks. See `docs/capability-vocabulary.md` for
//! the full rationale, rules, and backward-compatibility contract.
//!
//! Internal code should reference these constants rather than inlining the raw strings, so
//! a vocabulary change only requires updating this file and the companion module in Hymenium.

/// Rust compilation and Cargo tooling (build, test, clippy, fmt).
pub const RUST: &str = "rust";

/// React/TypeScript work (cap dashboard, npm build).
pub const FRONTEND: &str = "frontend";

/// JSON schema and septa contract work.
pub const SCHEMA: &str = "schema";

/// `SQLite` schema migrations and direct database work.
pub const SQLITE: &str = "sqlite";

/// Markdown authoring only (no compilation required).
pub const DOCS: &str = "docs";

/// Bash/zsh scripting.
pub const SHELL: &str = "shell";

/// Workflow runtime work (hymenium, canopy internals).
pub const ORCHESTRATION: &str = "orchestration";

/// All vocabulary labels in a stable slice — useful for validation and documentation.
pub const ALL: &[&str] = &[RUST, FRONTEND, SCHEMA, SQLITE, DOCS, SHELL, ORCHESTRATION];
