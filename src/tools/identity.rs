use serde_json::{Value, json};

use crate::models::{AgentRegistration, AgentRole, AgentStatus, SituationResult, WhoAmIResult};
use crate::store::CanopyStore;

use super::{ToolResult, get_str, get_string_array};

const TOOL_RESULT_SCHEMA_VERSION: &str = "1.0";

/// Register or update this agent.
pub fn tool_register(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let role = get_str(args, "role").and_then(|r| r.parse::<AgentRole>().ok());
    let capabilities = get_string_array(args, "capabilities");
    let model = get_str(args, "model").unwrap_or("unknown").to_string();
    let project_root = get_str(args, "project_root").unwrap_or("").to_string();
    let worktree_id = get_str(args, "worktree_id").unwrap_or("main").to_string();

    let host_id = get_str(args, "host_id").unwrap_or(agent_id).to_string();
    let host_type = get_str(args, "host_type")
        .unwrap_or("claude-code")
        .to_string();
    let host_instance = get_str(args, "host_instance")
        .unwrap_or(agent_id)
        .to_string();

    let registration = AgentRegistration {
        agent_id: agent_id.to_string(),
        host_id,
        host_type,
        host_instance,
        model,
        project_root,
        worktree_id,
        role,
        capabilities,
        status: AgentStatus::Idle,
        current_task_id: None,
        heartbeat_at: None,
    };

    match store.register_agent(&registration) {
        Ok(agent) => ToolResult::json(&agent),
        Err(err) => ToolResult::error(format!("registration failed: {err}")),
    }
}

/// Send heartbeat, get notifications.
pub fn tool_heartbeat(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let status = get_str(args, "status")
        .and_then(|s| s.parse::<AgentStatus>().ok())
        .unwrap_or(AgentStatus::Idle);
    let current_task_id = get_str(args, "current_task_id");

    match store.heartbeat_agent(agent_id, status, current_task_id) {
        Ok(_) => {}
        Err(err) => return ToolResult::error(format!("heartbeat failed: {err}")),
    }

    let pending_handoffs = match store.list_pending_handoffs_for(agent_id) {
        Ok(h) => h,
        Err(err) => return ToolResult::error(format!("failed to list handoffs: {err}")),
    };

    let stale_agents = match store.list_stale_agents(60) {
        Ok(a) => a,
        Err(err) => return ToolResult::error(format!("failed to list stale agents: {err}")),
    };

    ToolResult::json(&json!({
        "ack": true,
        "pending_handoffs": pending_handoffs,
        "stale_agents_count": stale_agents.len(),
    }))
}

/// Return full agent state.
pub fn tool_whoami(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    _args: &Value,
) -> ToolResult {
    let agent = match store.get_agent(agent_id) {
        Ok(a) => a,
        Err(err) => return ToolResult::error(format!("agent not found: {err}")),
    };

    let tasks = match store.list_tasks_for_agent(agent_id) {
        Ok(t) => t,
        Err(err) => return ToolResult::error(format!("failed to list tasks: {err}")),
    };

    let pending_handoffs = match store.list_pending_handoffs_for(agent_id) {
        Ok(h) => h,
        Err(err) => return ToolResult::error(format!("failed to list handoffs: {err}")),
    };

    let file_locks = match store.list_file_locks(None, Some(agent_id)) {
        Ok(l) => l,
        Err(err) => return ToolResult::error(format!("failed to list file locks: {err}")),
    };
    let mut workflow = Vec::with_capacity(tasks.len());
    for task in &tasks {
        match store.get_task_workflow_context(&task.task_id) {
            Ok(context) => workflow.push(context),
            Err(err) => {
                return ToolResult::error(format!(
                    "failed to load workflow context for task {}: {err}",
                    task.task_id
                ));
            }
        }
    }

    ToolResult::json(&WhoAmIResult {
        schema_version: TOOL_RESULT_SCHEMA_VERSION.to_string(),
        agent,
        tasks,
        workflow,
        pending_handoffs,
        file_locks,
    })
}

/// Situational awareness across all agents.
pub fn tool_situation(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let project_root = get_str(args, "project_root");

    let agents = match store.list_active_agents() {
        Ok(a) => a,
        Err(err) => return ToolResult::error(format!("failed to list agents: {err}")),
    };

    let file_locks = match store.list_file_locks(project_root, None) {
        Ok(l) => l,
        Err(err) => return ToolResult::error(format!("failed to list file locks: {err}")),
    };

    let open_handoffs = match store.list_handoffs(None) {
        Ok(h) => h
            .into_iter()
            .filter(|h| h.status == crate::models::HandoffStatus::Open)
            .count(),
        Err(err) => return ToolResult::error(format!("failed to list handoffs: {err}")),
    };
    let workflow = match store.list_task_workflow_contexts(project_root) {
        Ok(workflow) => workflow,
        Err(err) => return ToolResult::error(format!("failed to list workflow context: {err}")),
    };

    ToolResult::json(&SituationResult {
        schema_version: TOOL_RESULT_SCHEMA_VERSION.to_string(),
        agents,
        file_locks,
        workflow,
        open_handoffs_count: open_handoffs,
    })
}

#[cfg(test)]
mod tests {
    use super::tool_whoami;
    use crate::models::{AgentRegistration, AgentStatus};
    use crate::store::Store;
    use rusqlite::Connection;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn whoami_fails_fast_when_workflow_context_is_invalid() {
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
            .create_task("Assigned workflow", None, "operator", "/repo/demo", None)
            .expect("create task");
        store
            .assign_task(&task.task_id, &agent.agent_id, "operator", None)
            .expect("assign task");

        let conn = Connection::open(&db_path).expect("open sqlite");
        conn.execute(
            "UPDATE task_queue_states SET status = 'not-a-real-status' WHERE task_id = ?1",
            [task.task_id.as_str()],
        )
        .expect("corrupt queue state");

        let result = tool_whoami(&store, &agent.agent_id, &json!({}));
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
