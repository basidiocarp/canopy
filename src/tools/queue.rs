use serde_json::{Value, json};

use crate::models::WorkQueueResult;
use crate::store::{CLAIM_STALE_THRESHOLD_SECS, CanopyStore};

use super::{ToolResult, get_bounded_i64, get_str, validate_required_string};

const TOOL_RESULT_SCHEMA_VERSION: &str = "1.0";

/// List available tasks matching agent capabilities.
pub fn tool_work_queue(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
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
    let mut orchestration = Vec::with_capacity(tasks.len());
    for task in &tasks {
        match store.get_task_workflow_context(&task.task_id) {
            Ok(context) => orchestration.push(context),
            Err(err) => {
                return ToolResult::error(format!(
                    "failed to load workflow context for task {}: {err}",
                    task.task_id
                ));
            }
        }
    }

    ToolResult::json(&WorkQueueResult {
        schema_version: TOOL_RESULT_SCHEMA_VERSION.to_string(),
        available_tasks: tasks,
        orchestration,
        my_role: role_str,
        my_capabilities: capabilities,
    })
}

/// Atomically claim a task.
pub fn tool_task_claim(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(id) => id,
        Err(err) => return err,
    };
    let force_claim = args
        .get("force_claim")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if !force_claim {
        if let Err(err) =
            crate::store::ensure_agent_fresh_for_claim(store, agent_id, CLAIM_STALE_THRESHOLD_SECS)
        {
            return ToolResult::error(format!("claim failed: {err}"));
        }
    }

    match store.atomic_claim_task(agent_id, task_id) {
        Ok(Some(task)) => ToolResult::json(&task),
        Ok(None) => ToolResult::error(
            "Task already claimed by another agent (error 4001). Call canopy_work_queue to find available tasks.".to_string(),
        ),
        Err(err) => ToolResult::error(format!("claim failed: {err}")),
    }
}

/// Release a claimed task back to the open pool.
pub fn tool_task_yield(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
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
                return ToolResult::error(format!(
                    "yield status updated but failed to clear assignment: {err}"
                ));
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

#[cfg(test)]
mod tests {
    use super::tool_work_queue;
    use crate::models::{AgentRegistration, AgentStatus};
    use crate::store::Store;
    use rusqlite::Connection;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn work_queue_fails_fast_when_workflow_context_is_invalid() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("canopy.db");
        let store = Store::open(&db_path).expect("open store");
        let agent = AgentRegistration {
            agent_id: "agent-1".to_string(),
            host_id: "host-1".to_string(),
            host_type: "codex".to_string(),
            host_instance: "local".to_string(),
            model: "gpt-5.4".to_string(),
            project_root: "/repo/demo".to_string(),
            worktree_id: "wt-1".to_string(),
            role: None,
            capabilities: Vec::new(),
            status: AgentStatus::Idle,
            current_task_id: None,
            heartbeat_at: None,
        };
        store.register_agent(&agent).expect("register agent");
        let task = store
            .create_task("Broken workflow", None, "operator", "/repo/demo", None)
            .expect("create task");

        let conn = Connection::open(&db_path).expect("open sqlite");
        conn.execute(
            "UPDATE task_queue_states SET status = 'not-a-real-status' WHERE task_id = ?1",
            [task.task_id.as_str()],
        )
        .expect("corrupt queue state");

        let result = tool_work_queue(
            &store,
            &agent.agent_id,
            &json!({ "project_root": "/repo/demo" }),
        );
        assert!(result.is_error);
        assert!(
            result.content[0]
                .text
                .contains("failed to load workflow context"),
            "unexpected error: {}",
            result.content[0].text
        );
    }
}
