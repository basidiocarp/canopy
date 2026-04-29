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
    use crate::models::TaskStatus;

    let limit = get_bounded_i64(args, "limit", 5, 1, 20);
    let project_root = get_str(args, "project_root");
    let include_assigned = args
        .get("include_assigned")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // Get agent to know role/capabilities
    let (role_str, capabilities) = match store.get_agent(agent_id) {
        Ok(agent) => (
            agent.role.map(|r| r.to_string()),
            agent.capabilities.clone(),
        ),
        Err(_) => (None, Vec::new()),
    };

    let mut tasks = match store.query_available_tasks(
        role_str.as_deref(),
        &capabilities,
        project_root,
        limit,
    ) {
        Ok(t) => t,
        Err(err) => return ToolResult::error(format!("failed to query tasks: {err}")),
    };

    if include_assigned {
        let assigned_tasks = match store.list_tasks_for_agent(agent_id) {
            Ok(t) => t,
            Err(err) => return ToolResult::error(format!("failed to query assigned tasks: {err}")),
        };

        // Filter assigned tasks to active statuses (exclude Completed, Closed, Cancelled)
        let active_assigned: Vec<_> = assigned_tasks
            .into_iter()
            .filter(|task| {
                !matches!(
                    task.status,
                    TaskStatus::Completed | TaskStatus::Closed | TaskStatus::Cancelled
                )
            })
            .collect();

        // Merge with claimable tasks, deduplicating by task_id
        let mut seen_ids = std::collections::HashSet::new();
        for task in &tasks {
            seen_ids.insert(task.task_id.clone());
        }

        for task in active_assigned {
            if !seen_ids.contains(&task.task_id) {
                seen_ids.insert(task.task_id.clone());
                tasks.push(task);
            }
        }
    }

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
///
/// Enforces capability requirements: if the task has non-empty
/// `required_capabilities`, the claiming agent must have all of them.
/// Returns a clear error listing the missing capabilities on mismatch.
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

    // Capability check: load task and agent, then verify requirements are met.
    if let Err(err) = crate::store::ensure_capabilities_match(store, task_id, agent_id) {
        return ToolResult::error(format!("claim failed: {err}"));
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
    use super::{tool_task_claim, tool_work_queue};
    use crate::models::{AgentRegistration, AgentStatus, TaskStatus};
    use crate::store::Store;
    use crate::store::TaskCreationOptions;
    use rusqlite::Connection;
    use serde_json::json;
    use tempfile::tempdir;

    fn open_store() -> (Store, tempfile::TempDir) {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("canopy.db");
        let store = Store::open(&db_path).expect("open store");
        (store, temp)
    }

    fn register_agent_with_caps(
        store: &Store,
        agent_id: &str,
        caps: Vec<String>,
    ) -> AgentRegistration {
        let agent = AgentRegistration {
            agent_id: agent_id.to_string(),
            host_id: "host-1".to_string(),
            host_type: "codex".to_string(),
            host_instance: "local".to_string(),
            model: "sonnet".to_string(),
            project_root: "/repo".to_string(),
            worktree_id: "wt-1".to_string(),
            role: None,
            capabilities: caps,
            status: AgentStatus::Idle,
            current_task_id: None,
            heartbeat_at: None,
        };
        store.register_agent(&agent).expect("register agent")
    }

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

    // -- capability enforcement tests -------------------------------------------

    #[test]
    fn claim_succeeds_when_agent_has_required_capabilities() {
        let (store, _temp) = open_store();
        let agent = register_agent_with_caps(&store, "cap-agent-1", vec!["rust".to_string()]);
        // Give the agent a recent heartbeat so the freshness check passes.
        store
            .heartbeat_agent(&agent.agent_id, AgentStatus::Idle, None)
            .expect("heartbeat");

        let task = store
            .create_task_with_options(
                "Rust work",
                None,
                "operator",
                "/repo",
                &TaskCreationOptions {
                    required_capabilities: vec!["rust".to_string()],
                    ..Default::default()
                },
            )
            .expect("create task");

        let result = tool_task_claim(&store, &agent.agent_id, &json!({ "task_id": task.task_id }));
        assert!(
            !result.is_error,
            "claim should succeed when agent has required caps; error: {}",
            result.content[0].text
        );
    }

    #[test]
    fn claim_fails_with_capability_mismatch_error() {
        let (store, _temp) = open_store();
        // Agent declares "frontend" capability but task requires "rust".
        // An agent that has declared capabilities must have the right ones.
        let agent =
            register_agent_with_caps(&store, "wrong-caps-agent", vec!["frontend".to_string()]);
        store
            .heartbeat_agent(&agent.agent_id, AgentStatus::Idle, None)
            .expect("heartbeat");

        let task = store
            .create_task_with_options(
                "Rust work requiring capability",
                None,
                "operator",
                "/repo",
                &TaskCreationOptions {
                    required_capabilities: vec!["rust".to_string()],
                    ..Default::default()
                },
            )
            .expect("create task");

        let result = tool_task_claim(&store, &agent.agent_id, &json!({ "task_id": task.task_id }));
        assert!(result.is_error, "claim should fail on capability mismatch");
        assert!(
            result.content[0].text.contains("capability mismatch"),
            "error should mention capability mismatch, got: {}",
            result.content[0].text
        );
        assert!(
            result.content[0].text.contains("missing"),
            "error should list missing capabilities, got: {}",
            result.content[0].text
        );
    }

    #[test]
    fn claim_fails_listing_missing_capability_when_agent_has_partial_match() {
        let (store, _temp) = open_store();
        // Agent has "rust" but task requires "rust" + "shell".
        let agent =
            register_agent_with_caps(&store, "partial-caps-agent", vec!["rust".to_string()]);
        store
            .heartbeat_agent(&agent.agent_id, AgentStatus::Idle, None)
            .expect("heartbeat");

        let task = store
            .create_task_with_options(
                "Multi-capability task",
                None,
                "operator",
                "/repo",
                &TaskCreationOptions {
                    required_capabilities: vec!["rust".to_string(), "shell".to_string()],
                    ..Default::default()
                },
            )
            .expect("create task");

        let result = tool_task_claim(&store, &agent.agent_id, &json!({ "task_id": task.task_id }));
        assert!(
            result.is_error,
            "claim should fail on partial capability mismatch"
        );
        assert!(
            result.content[0].text.contains("shell"),
            "error should list shell as missing, got: {}",
            result.content[0].text
        );
    }

    #[test]
    fn claim_succeeds_when_task_has_no_required_capabilities() {
        let (store, _temp) = open_store();
        // Agent has no capabilities, but task has no requirements — should succeed.
        let agent = register_agent_with_caps(&store, "bare-agent", vec![]);
        store
            .heartbeat_agent(&agent.agent_id, AgentStatus::Idle, None)
            .expect("heartbeat");

        let task = store
            .create_task("No capability task", None, "operator", "/repo", None)
            .expect("create task");

        let result = tool_task_claim(&store, &agent.agent_id, &json!({ "task_id": task.task_id }));
        assert!(
            !result.is_error,
            "claim should succeed for no-requirement task; error: {}",
            result.content[0].text
        );
    }

    // -- include_assigned tests ------------------------------------------------

    #[test]
    fn assigned_task_not_visible_in_default_work_queue() {
        let (store, _temp) = open_store();
        let agent = register_agent_with_caps(&store, "assigned-agent", vec![]);
        store
            .heartbeat_agent(&agent.agent_id, AgentStatus::Idle, None)
            .expect("heartbeat");

        let task = store
            .create_task("Assigned task", None, "operator", "/repo", None)
            .expect("create task");

        // Assign the task to the agent
        store
            .assign_task(&task.task_id, &agent.agent_id, "operator", None)
            .expect("assign task");

        // Query work queue without include_assigned - should not see the assigned task
        let result = tool_work_queue(
            &store,
            &agent.agent_id,
            &json!({ "include_assigned": false }),
        );
        assert!(
            !result.is_error,
            "work_queue should succeed; error: {}",
            result.content[0].text
        );
        // Parse the response to check task count
        let response_text = &result.content[0].text;
        let parsed: serde_json::Value =
            serde_json::from_str(response_text).expect("response should be valid JSON");
        let available_tasks = parsed["available_tasks"].as_array().expect("tasks array");
        assert_eq!(
            available_tasks.len(),
            0,
            "assigned task should not appear in default work queue"
        );
    }

    #[test]
    fn assigned_task_visible_with_include_assigned_flag() {
        let (store, _temp) = open_store();
        let agent = register_agent_with_caps(&store, "assigned-agent-2", vec![]);
        store
            .heartbeat_agent(&agent.agent_id, AgentStatus::Idle, None)
            .expect("heartbeat");

        let task = store
            .create_task("Assigned task 2", None, "operator", "/repo", None)
            .expect("create task");

        // Assign the task to the agent
        store
            .assign_task(&task.task_id, &agent.agent_id, "operator", None)
            .expect("assign task");

        // Query work queue with include_assigned=true - should see the assigned task
        let result = tool_work_queue(
            &store,
            &agent.agent_id,
            &json!({ "include_assigned": true }),
        );
        assert!(
            !result.is_error,
            "work_queue should succeed; error: {}",
            result.content[0].text
        );
        let response_text = &result.content[0].text;
        let parsed: serde_json::Value =
            serde_json::from_str(response_text).expect("response should be valid JSON");
        let available_tasks = parsed["available_tasks"].as_array().expect("tasks array");
        assert_eq!(
            available_tasks.len(),
            1,
            "assigned task should appear in work queue with include_assigned=true"
        );
        assert_eq!(
            available_tasks[0]["task_id"].as_str(),
            Some(task.task_id.as_str()),
            "the returned task should match the assigned task"
        );
    }

    #[test]
    fn claimable_tasks_appear_in_both_modes() {
        let (store, _temp) = open_store();
        let agent = register_agent_with_caps(&store, "queue-agent", vec![]);
        store
            .heartbeat_agent(&agent.agent_id, AgentStatus::Idle, None)
            .expect("heartbeat");

        // Create an open (claimable) task
        let open_task = store
            .create_task("Open task", None, "operator", "/repo", None)
            .expect("create task");

        // Create and assign another task
        let assigned_task = store
            .create_task("Assigned task", None, "operator", "/repo", None)
            .expect("create task");
        store
            .assign_task(&assigned_task.task_id, &agent.agent_id, "operator", None)
            .expect("assign task");

        // Default mode should only show the open task
        let result_default = tool_work_queue(
            &store,
            &agent.agent_id,
            &json!({ "include_assigned": false }),
        );
        let response_default = &result_default.content[0].text;
        let parsed_default: serde_json::Value =
            serde_json::from_str(response_default).expect("valid JSON");
        let default_tasks = parsed_default["available_tasks"].as_array().expect("tasks");
        assert_eq!(
            default_tasks.len(),
            1,
            "default mode should show only claimable task"
        );
        assert_eq!(
            default_tasks[0]["task_id"].as_str(),
            Some(open_task.task_id.as_str())
        );

        // Mode with include_assigned=true should show both
        let result_include = tool_work_queue(
            &store,
            &agent.agent_id,
            &json!({ "include_assigned": true }),
        );
        let response_include = &result_include.content[0].text;
        let parsed_include: serde_json::Value =
            serde_json::from_str(response_include).expect("valid JSON");
        let include_tasks = parsed_include["available_tasks"].as_array().expect("tasks");
        assert_eq!(
            include_tasks.len(),
            2,
            "include_assigned=true should show both claimable and assigned tasks"
        );
    }

    #[test]
    fn completed_assigned_task_excluded_with_include_assigned() {
        let (store, _temp) = open_store();
        let agent = register_agent_with_caps(&store, "terminal-agent", vec![]);
        store
            .heartbeat_agent(&agent.agent_id, AgentStatus::Idle, None)
            .expect("heartbeat");

        let task = store
            .create_task("Terminal task", None, "operator", "/repo", None)
            .expect("create task");

        // Assign, start, then complete the task so it reaches a terminal status.
        // The state machine requires: assigned → in_progress → completed.
        store
            .assign_task(&task.task_id, &agent.agent_id, "operator", None)
            .expect("assign task");
        store
            .update_task_status(
                &task.task_id,
                TaskStatus::InProgress,
                "operator",
                crate::store::TaskStatusUpdate::default(),
            )
            .expect("start task");
        store
            .update_task_status(
                &task.task_id,
                TaskStatus::Completed,
                "operator",
                crate::store::TaskStatusUpdate::default(),
            )
            .expect("complete task");

        // Completed tasks must not appear even with include_assigned=true
        let result = tool_work_queue(
            &store,
            &agent.agent_id,
            &json!({ "include_assigned": true }),
        );
        assert!(
            !result.is_error,
            "work_queue should succeed; error: {}",
            result.content[0].text
        );
        let response_text = &result.content[0].text;
        let parsed: serde_json::Value =
            serde_json::from_str(response_text).expect("response should be valid JSON");
        let available_tasks = parsed["available_tasks"].as_array().expect("tasks array");
        assert_eq!(
            available_tasks.len(),
            0,
            "completed task must be excluded even with include_assigned=true"
        );
    }
}
