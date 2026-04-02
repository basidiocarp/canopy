// Tools exported from this module:
// - tool_council_post
// - tool_council_show

use crate::models::CouncilMessageType;
use crate::store::CanopyStore;
use crate::tools::{ToolResult, validate_required_string};
use serde_json::Value;
use std::str::FromStr;

/// Post a message to a task's council thread.
pub fn tool_council_post(store: &(impl CanopyStore + ?Sized), agent_id: &str, args: &Value) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let message_type_str = match validate_required_string(args, "message_type") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Ok(message_type) = CouncilMessageType::from_str(message_type_str) else {
        return ToolResult::error(format!("invalid message_type: {message_type_str}"));
    };
    let body = match validate_required_string(args, "body") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match store.add_council_message(task_id, agent_id, message_type, body) {
        Ok(message) => ToolResult::json(&message),
        Err(e) => ToolResult::error(format!("failed to post council message: {e}")),
    }
}

/// Read the council thread for a task.
pub fn tool_council_show(store: &(impl CanopyStore + ?Sized), _agent_id: &str, args: &Value) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match store.list_council_messages(task_id) {
        Ok(messages) => ToolResult::json(&messages),
        Err(e) => ToolResult::error(format!("failed to list council messages: {e}")),
    }
}
