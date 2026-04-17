//! Tools for the orchestration outcome learning loop.
//!
//! Provides four MCP tools:
//!
//! - `canopy_outcome_record` — parse and store a `workflow-outcome-v1` JSON blob
//! - `canopy_outcome_list`   — list all stored outcomes, most recent first
//! - `canopy_outcome_show`   — retrieve a single outcome by `workflow_id`
//! - `canopy_outcome_summary` — counts grouped by template, failure type, and last phase
//!
//! All surfaces are **observational only** — they record and query what happened.
//! They do not auto-modify routing policy.

use serde_json::Value;

use crate::store::CanopyStore;
use crate::tools::{ToolResult, validate_required_string};

/// `canopy_outcome_record`: parse and store a `workflow-outcome-v1` JSON blob.
///
/// Accepts either a `json` string argument containing the full payload, or a
/// `json_object` argument which is the parsed object itself (forwarded as
/// re-serialised JSON).
pub fn tool_outcome_record(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    // Accept either a string-valued "json" key or a direct "json_object".
    let raw: Vec<u8> = if let Some(s) = args.get("json").and_then(Value::as_str) {
        s.as_bytes().to_vec()
    } else if let Some(obj) = args.get("json_object") {
        match serde_json::to_vec(obj) {
            Ok(b) => b,
            Err(e) => return ToolResult::error(format!("failed to re-serialise json_object: {e}")),
        }
    } else {
        return ToolResult::error(
            "missing required parameter: provide 'json' (string) or 'json_object' (object)"
                .to_string(),
        );
    };

    match store.insert_workflow_outcome(&raw) {
        Ok(record) => ToolResult::json(&record),
        Err(e) => ToolResult::error(format!("failed to record outcome: {e}")),
    }
}

/// `canopy_outcome_list`: list all stored outcomes, most recent first.
pub fn tool_outcome_list(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    _args: &Value,
) -> ToolResult {
    match store.list_workflow_outcomes() {
        Ok(records) => ToolResult::json(&records),
        Err(e) => ToolResult::error(format!("failed to list outcomes: {e}")),
    }
}

/// `canopy_outcome_show`: retrieve a single outcome by `workflow_id`.
pub fn tool_outcome_show(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let workflow_id = match validate_required_string(args, "workflow_id") {
        Ok(s) => s,
        Err(err) => return err,
    };
    match store.get_workflow_outcome(workflow_id) {
        Ok(Some(record)) => ToolResult::json(&record),
        Ok(None) => ToolResult::error(format!("outcome not found for workflow_id: {workflow_id}")),
        Err(e) => ToolResult::error(format!("failed to get outcome: {e}")),
    }
}

/// `canopy_outcome_summary`: counts grouped by template, failure type, and last phase.
///
/// Returns a JSON array of `OutcomeSummaryRow` objects. This surface is
/// **observational** — it does not auto-modify routing policy.
pub fn tool_outcome_summary(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    _args: &Value,
) -> ToolResult {
    match store.outcome_summary_by_template_failure() {
        Ok(rows) => ToolResult::json(&rows),
        Err(e) => ToolResult::error(format!("failed to compute outcome summary: {e}")),
    }
}
