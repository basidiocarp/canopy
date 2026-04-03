pub mod api;
pub mod cli;
pub mod handoff_check;
pub mod mcp;
pub mod models;
pub mod scope;
pub mod store;
pub mod tools;

// Re-export completeness check for use in task completion gates.
pub use handoff_check::check_completeness;
