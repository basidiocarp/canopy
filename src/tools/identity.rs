use serde_json::{Value, json};

use crate::models::{AgentRegistration, AgentRole, AgentStatus};
use crate::store::CanopyStore;

use super::{ToolResult, get_str, get_string_array};

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

    ToolResult::json(&json!({
        "agent": agent,
        "tasks": tasks,
        "pending_handoffs": pending_handoffs,
        "file_locks": file_locks,
    }))
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

    ToolResult::json(&json!({
        "agents": agents,
        "file_locks": file_locks,
        "open_handoffs_count": open_handoffs,
    }))
}
