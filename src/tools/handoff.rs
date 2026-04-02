// Tools exported from this module:
// - tool_handoff_create
// - tool_handoff_accept
// - tool_handoff_reject
// - tool_handoff_complete
// - tool_handoff_list

use crate::models::{HandoffStatus, HandoffType};
use crate::store::{CanopyStore, HandoffTiming};
use crate::tools::{ToolResult, get_str, validate_required_string};
use serde_json::Value;
use std::str::FromStr;

/// Create a new handoff for a task.
pub fn tool_handoff_create(store: &(impl CanopyStore + ?Sized), agent_id: &str, args: &Value) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let handoff_type_str = match validate_required_string(args, "handoff_type") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Ok(handoff_type) = HandoffType::from_str(handoff_type_str) else {
        return ToolResult::error(format!("invalid handoff_type: {handoff_type_str}"));
    };
    let summary = match validate_required_string(args, "summary") {
        Ok(v) => v,
        Err(e) => return e,
    };

    // to_agent_id is required (either directly or as to_role target)
    let to_agent_id = match validate_required_string(args, "to_agent_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let requested_action = get_str(args, "requested_action");
    let timing = HandoffTiming {
        due_at: get_str(args, "due_at"),
        expires_at: get_str(args, "expires_at"),
    };

    match store.create_handoff(task_id, agent_id, to_agent_id, handoff_type, summary, requested_action, timing) {
        Ok(handoff) => ToolResult::json(&handoff),
        Err(e) => ToolResult::error(format!("failed to create handoff: {e}")),
    }
}

/// Accept a pending handoff.
pub fn tool_handoff_accept(store: &(impl CanopyStore + ?Sized), agent_id: &str, args: &Value) -> ToolResult {
    let handoff_id = match validate_required_string(args, "handoff_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match store.resolve_handoff_with_actor(handoff_id, HandoffStatus::Accepted, agent_id, Some(agent_id)) {
        Ok(handoff) => ToolResult::json(&handoff),
        Err(e) => ToolResult::error(format!("failed to accept handoff: {e}")),
    }
}

/// Reject a pending handoff.
pub fn tool_handoff_reject(store: &(impl CanopyStore + ?Sized), agent_id: &str, args: &Value) -> ToolResult {
    let handoff_id = match validate_required_string(args, "handoff_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match store.resolve_handoff_with_actor(handoff_id, HandoffStatus::Rejected, agent_id, Some(agent_id)) {
        Ok(handoff) => ToolResult::json(&handoff),
        Err(e) => ToolResult::error(format!("failed to reject handoff: {e}")),
    }
}

/// Complete a handoff after finishing the work.
pub fn tool_handoff_complete(store: &(impl CanopyStore + ?Sized), agent_id: &str, args: &Value) -> ToolResult {
    let handoff_id = match validate_required_string(args, "handoff_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match store.resolve_handoff(handoff_id, HandoffStatus::Completed, agent_id) {
        Ok(handoff) => ToolResult::json(&handoff),
        Err(e) => ToolResult::error(format!("failed to complete handoff: {e}")),
    }
}

/// List handoffs filtered by schema-defined parameters: `task_id`, `to_agent_id`,
/// `from_agent_id`, `status`. With no filters, defaults to pending handoffs for
/// this agent.
pub fn tool_handoff_list(store: &(impl CanopyStore + ?Sized), agent_id: &str, args: &Value) -> ToolResult {
    let task_id = get_str(args, "task_id");
    let to_agent = get_str(args, "to_agent_id");
    let from_agent = get_str(args, "from_agent_id");
    let status_filter = get_str(args, "status")
        .and_then(|s| HandoffStatus::from_str(s).ok());

    // If a task_id is provided, scope to that task
    if let Some(tid) = task_id {
        match store.list_handoffs(Some(tid)) {
            Ok(mut handoffs) => {
                if let Some(to) = to_agent {
                    handoffs.retain(|h| h.to_agent_id == to);
                }
                if let Some(from) = from_agent {
                    handoffs.retain(|h| h.from_agent_id == from);
                }
                if let Some(ref status) = status_filter {
                    handoffs.retain(|h| &h.status == status);
                }
                ToolResult::json(&handoffs)
            }
            Err(e) => ToolResult::error(format!("failed to list handoffs: {e}")),
        }
    } else if to_agent.is_some() || from_agent.is_some() || status_filter.is_some() {
        // No task_id but other filters present — list all and post-filter
        match store.list_handoffs(None) {
            Ok(mut handoffs) => {
                if let Some(to) = to_agent {
                    handoffs.retain(|h| h.to_agent_id == to);
                }
                if let Some(from) = from_agent {
                    handoffs.retain(|h| h.from_agent_id == from);
                }
                if let Some(ref status) = status_filter {
                    handoffs.retain(|h| &h.status == status);
                }
                ToolResult::json(&handoffs)
            }
            Err(e) => ToolResult::error(format!("failed to list handoffs: {e}")),
        }
    } else {
        // No filters at all — default to pending handoffs for this agent
        match store.list_pending_handoffs_for(agent_id) {
            Ok(handoffs) => ToolResult::json(&handoffs),
            Err(e) => ToolResult::error(format!("failed to list handoffs: {e}")),
        }
    }
}
