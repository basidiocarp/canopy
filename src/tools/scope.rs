use serde::Serialize;
use serde_json::Value;

use super::{ToolResult, validate_required_string};
use crate::runtime::handle_scope_gap;
use crate::store::CanopyStore;

#[derive(Debug, Serialize)]
struct HandoffScope {
    task_id: String,
    parent_task_id: Option<String>,
    title: String,
    status: String,
    scope: Vec<String>,
}

/// Report a scope gap and apply the scope-gap protocol.
pub fn tool_report_scope_gap(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let work_item = match validate_required_string(args, "work_item") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match handle_scope_gap(store, task_id, agent_id, work_item) {
        Ok(outcome) => ToolResult::json(&outcome),
        Err(err) => ToolResult::error(format!("failed to report scope gap: {err}")),
    }
}

/// Retrieve the declared handoff scope for a task.
pub fn tool_get_handoff_scope(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match store.get_task(task_id) {
        Ok(task) => ToolResult::json(&HandoffScope {
            task_id: task.task_id,
            parent_task_id: task.parent_task_id,
            title: task.title,
            status: task.status.to_string(),
            scope: task.scope,
        }),
        Err(err) => ToolResult::error(format!("failed to get handoff scope: {err}")),
    }
}
