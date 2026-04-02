use serde_json::{Value, json};

use crate::store::CanopyStore;

use super::{ToolResult, get_bounded_i64, get_str, validate_required_string};

/// List available tasks matching agent capabilities.
pub fn tool_work_queue(store: &(impl CanopyStore + ?Sized), agent_id: &str, args: &Value) -> ToolResult {
    let limit = get_bounded_i64(args, "limit", 5, 1, 20);
    let project_root = get_str(args, "project_root");

    // Get agent to know role/capabilities
    let (role_str, capabilities) = match store.get_agent(agent_id) {
        Ok(agent) => (
            agent.role.map(|r| r.to_string()),
            agent.capabilities.clone(),
        ),
        Err(_) => (None, Vec::new()),
    };

    let tasks = match store.query_available_tasks(
        role_str.as_deref(),
        &capabilities,
        project_root,
        limit,
    ) {
        Ok(t) => t,
        Err(err) => return ToolResult::error(format!("failed to query tasks: {err}")),
    };

    ToolResult::json(&json!({
        "available_tasks": tasks,
        "my_role": role_str,
        "my_capabilities": capabilities,
    }))
}

/// Atomically claim a task.
pub fn tool_task_claim(store: &(impl CanopyStore + ?Sized), agent_id: &str, args: &Value) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(id) => id,
        Err(err) => return err,
    };

    match store.atomic_claim_task(agent_id, task_id) {
        Ok(Some(task)) => ToolResult::json(&task),
        Ok(None) => ToolResult::error(
            "Task already claimed by another agent (error 4001). Call canopy_work_queue to find available tasks.".to_string(),
        ),
        Err(err) => ToolResult::error(format!("claim failed: {err}")),
    }
}

/// Release a claimed task back to the open pool.
pub fn tool_task_yield(store: &(impl CanopyStore + ?Sized), agent_id: &str, args: &Value) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(id) => id,
        Err(err) => return err,
    };
    let reason = get_str(args, "reason");

    // Verify task is assigned to this agent
    let task = match store.get_task(task_id) {
        Ok(t) => t,
        Err(err) => return ToolResult::error(format!("task not found: {err}")),
    };

    if task.owner_agent_id.as_deref() != Some(agent_id) {
        return ToolResult::error(format!(
            "task {task_id} is not assigned to agent {agent_id}"
        ));
    }

    // Update task back to open status with no owner
    match store.update_task_status(
        task_id,
        crate::models::TaskStatus::Open,
        agent_id,
        crate::store::TaskStatusUpdate {
            event_note: reason,
            ..Default::default()
        },
    ) {
        Ok(_) => {
            // update_task_status doesn't clear owner_agent_id, so yielded tasks
            // remain invisible to query_available_tasks (which filters on
            // owner_agent_id IS NULL). Clear the assignment explicitly.
            if let Err(err) = store.clear_task_assignment(task_id) {
                return ToolResult::error(format!("yield status updated but failed to clear assignment: {err}"));
            }
            ToolResult::json(&json!({
                "ack": true,
                "task_id": task_id,
                "reason": reason,
            }))
        }
        Err(err) => ToolResult::error(format!("yield failed: {err}")),
    }
}
