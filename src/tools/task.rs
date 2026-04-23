// Tools exported from this module:
// - tool_task_create
// - tool_task_decompose
// - tool_task_get
// - tool_task_list
// - tool_task_update_status
// - tool_task_complete
// - tool_task_block
// - tool_task_snapshot

use crate::api::{self, SnapshotOptions};
use crate::models::{
    AgentRole, SnapshotPreset, TaskAction, TaskPriority, TaskRelationshipKind,
    TaskRelationshipRole, TaskSeverity, TaskStatus,
};
use crate::store::{
    CanopyStore, EvidenceLinkRefs, TaskCreationOptions, TaskGetStore, TaskStatusUpdate,
};
use crate::tools::{ToolResult, get_bool, get_str, get_string_array, validate_required_string};
use serde::Serialize;
use serde_json::Value;
use std::fmt::Write;
use std::str::FromStr;

/// Create a new task.
pub fn tool_task_create(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let title = match validate_required_string(args, "title") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let description = get_str(args, "description");
    let project_root = get_str(args, "project_root").unwrap_or(".");
    let required_role = get_str(args, "required_role").and_then(|s| AgentRole::from_str(s).ok());
    let required_capabilities = get_string_array(args, "required_capabilities");
    let verification_required = args
        .get("verification_required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let workflow_id = get_str(args, "workflow_id").map(ToOwned::to_owned);
    let phase_id = get_str(args, "phase_id").map(ToOwned::to_owned);

    let options = TaskCreationOptions {
        required_role,
        required_capabilities,
        verification_required,
        workflow_id,
        phase_id,
        ..TaskCreationOptions::default()
    };

    match store.create_task_with_options(title, description, agent_id, project_root, &options) {
        Ok(task) => ToolResult::json(&task),
        Err(e) => ToolResult::error(format!("failed to create task: {e}")),
    }
}

#[derive(Debug, Serialize)]
struct SubtaskCreated {
    task_id: String,
    title: String,
    blocked_by: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DecomposeResult {
    parent_task_id: String,
    subtasks: Vec<SubtaskCreated>,
}

/// Create subtasks from a parent task.
pub fn tool_task_decompose(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let parent_task_id = match validate_required_string(args, "parent_task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let Some(subtasks_value) = args.get("subtasks").and_then(Value::as_array) else {
        return ToolResult::error("missing required parameter: subtasks".to_string());
    };

    let mut created: Vec<SubtaskCreated> = Vec::new();

    for item in subtasks_value {
        let Some(title) = item.get("title").and_then(Value::as_str) else {
            return ToolResult::error("each subtask requires a title".to_string());
        };
        let description = item.get("description").and_then(Value::as_str);
        let required_role = item
            .get("role")
            .and_then(Value::as_str)
            .and_then(|s| AgentRole::from_str(s).ok());
        let files = item
            .get("files")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let options = TaskCreationOptions {
            required_role,
            required_capabilities: files,
            ..TaskCreationOptions::default()
        };

        let task = match store.create_subtask_with_options(
            parent_task_id,
            title,
            description,
            agent_id,
            &options,
        ) {
            Ok(t) => t,
            Err(e) => return ToolResult::error(format!("failed to create subtask: {e}")),
        };

        // Resolve blocked_by based on depends_on_index
        // Note: Each subtask supports at most one dependency (depends_on_index is a single integer, not an array).
        // This is intentional: decomposition creates a dependency chain, not a DAG.
        let mut blocked_by = Vec::new();
        if let Some(dep_index) = item.get("depends_on_index").and_then(Value::as_u64) {
            if let Some(dep) = usize::try_from(dep_index).ok().and_then(|i| created.get(i)) {
                blocked_by.push(dep.task_id.clone());
                // Persist the Blocks relationship: prior subtask (source) blocks new subtask (target)
                if let Err(e) = store.add_task_relationship(
                    &dep.task_id,
                    &task.task_id,
                    TaskRelationshipKind::Blocks,
                    agent_id,
                ) {
                    return ToolResult::error(format!(
                        "failed to persist dependency relationship: {e}"
                    ));
                }
            }
        }

        created.push(SubtaskCreated {
            task_id: task.task_id,
            title: task.title,
            blocked_by,
        });
    }

    let result = DecomposeResult {
        parent_task_id: parent_task_id.to_string(),
        subtasks: created,
    };
    ToolResult::json(&result)
}

/// Get task detail by ID.
pub fn tool_task_get(
    store: &(impl TaskGetStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match store.get_task(task_id) {
        Ok(task) => ToolResult::json(&task),
        Err(e) => ToolResult::error(format!("failed to get task: {e}")),
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::models::{Task, TaskPriority, TaskSeverity, VerificationState};
    use crate::store::{StoreError, StoreResult};
    use serde_json::json;

    struct MockTaskLookupStore {
        tasks: Vec<Task>,
    }

    impl TaskGetStore for MockTaskLookupStore {
        fn get_task(&self, task_id: &str) -> StoreResult<Task> {
            self.tasks
                .iter()
                .find(|task| task.task_id == task_id)
                .cloned()
                .ok_or(StoreError::NotFound("task"))
        }
    }

    fn make_task(id: &str, title: &str) -> Task {
        Task {
            task_id: id.to_string(),
            title: title.to_string(),
            description: None,
            requested_by: "test".to_string(),
            project_root: ".".to_string(),
            parent_task_id: None,
            queue_state_id: None,
            worktree_binding_id: None,
            execution_session_ref: None,
            review_cycle_id: None,
            workflow_id: None,
            phase_id: None,
            required_role: None,
            required_capabilities: Vec::new(),
            auto_review: false,
            verification_required: false,
            status: TaskStatus::Open,
            verification_state: VerificationState::Unknown,
            priority: TaskPriority::Medium,
            severity: TaskSeverity::None,
            owner_agent_id: None,
            owner_note: None,
            acknowledged_by: None,
            acknowledged_at: None,
            blocked_reason: None,
            verified_by: None,
            verified_at: None,
            closed_by: None,
            closure_summary: None,
            closed_at: None,
            due_at: None,
            review_due_at: None,
            scope: Vec::new(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn tool_task_get_uses_minimal_lookup_store() {
        let store = MockTaskLookupStore {
            tasks: vec![make_task("task-1", "Test task")],
        };

        let result = tool_task_get(&store, "agent-1", &json!({ "task_id": "task-1" }));
        assert!(!result.is_error);
        assert_eq!(result.content.len(), 1);
        assert!(result.content[0].text.contains("task-1"));
    }
}

/// List tasks with optional filters.
pub fn tool_task_list(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let assigned_to = get_str(args, "assigned_to");
    let project_root = get_str(args, "project_root");

    // If listing by assigned agent
    if assigned_to.is_some() || args.get("preset").and_then(Value::as_str) == Some("mine") {
        let lookup_agent = assigned_to.unwrap_or(agent_id);
        match store.list_tasks_for_agent(lookup_agent) {
            Ok(tasks) => {
                let mut filtered = tasks;
                if let Some(pr) = project_root {
                    filtered.retain(|t| t.project_root == pr);
                }
                if let Some(status_str) = get_str(args, "status") {
                    if let Ok(status) = TaskStatus::from_str(status_str) {
                        filtered.retain(|t| t.status == status);
                    }
                }
                return ToolResult::json(&filtered);
            }
            Err(e) => return ToolResult::error(format!("failed to list tasks: {e}")),
        }
    }

    // Default: list all tasks with optional filters
    match store.list_tasks() {
        Ok(tasks) => {
            let mut filtered = tasks;
            if let Some(pr) = project_root {
                filtered.retain(|t| t.project_root == pr);
            }
            if let Some(status_str) = get_str(args, "status") {
                if let Ok(status) = TaskStatus::from_str(status_str) {
                    filtered.retain(|t| t.status == status);
                }
            }
            ToolResult::json(&filtered)
        }
        Err(e) => ToolResult::error(format!("failed to list tasks: {e}")),
    }
}

/// Transition task status.
pub fn tool_task_update_status(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let status_str = match validate_required_string(args, "status") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Ok(status) = TaskStatus::from_str(status_str) else {
        return ToolResult::error(format!("invalid status: {status_str}"));
    };
    let reason = get_str(args, "reason");

    let update = TaskStatusUpdate {
        blocked_reason: if status == TaskStatus::Blocked {
            reason
        } else {
            None
        },
        event_note: reason,
        ..TaskStatusUpdate::default()
    };

    match store.update_task_status(task_id, status, agent_id, update) {
        Ok(task) => ToolResult::json(&task),
        Err(e) => ToolResult::error(format!("failed to update task status: {e}")),
    }
}

/// Mark task complete with evidence.
///
/// If `handoff_path` is provided, validates that the handoff document meets
/// completion criteria before allowing the transition. Tasks without a
/// handoff path bypass the check for backward compatibility.
///
/// If `verification_required=true`, checks for passing `ScriptVerification` evidence
/// before allowing completion. Can be overridden with `--force`.
#[allow(clippy::too_many_lines)]
pub fn tool_task_complete(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let summary = match validate_required_string(args, "summary") {
        Ok(v) => v,
        Err(e) => return e,
    };

    // Gate: if a handoff path is provided, check completeness first
    if let Some(handoff_path_str) = get_str(args, "handoff_path") {
        let handoff_path = std::path::Path::new(handoff_path_str);
        match crate::handoff_check::check_completeness(handoff_path) {
            Ok(report) => {
                if !report.is_complete {
                    return ToolResult::error(format!(
                        "completion rejected: {}",
                        crate::handoff_check::format_incomplete_report(&report)
                    ));
                }
            }
            Err(e) => {
                return ToolResult::error(format!("failed to check handoff completeness: {e}"));
            }
        }
    }

    // Gate: if verification_required=true, check for passing ScriptVerification evidence
    let force = get_bool(args, "force").unwrap_or(false);
    let task_record = match store.get_task(task_id) {
        Ok(t) => t,
        Err(e) => return ToolResult::error(format!("failed to load task: {e}")),
    };

    if task_record.verification_required && !force {
        let evidence: Vec<_> = store.list_evidence(task_id).unwrap_or_default();
        let has_passing_verification = evidence.iter().any(|e| {
            matches!(
                e.source_kind,
                crate::models::EvidenceSourceKind::ScriptVerification
            ) && e
                .summary
                .as_deref()
                .is_some_and(|s| s.contains("script verification passed"))
        });
        if !has_passing_verification {
            return ToolResult::error(format!(
                "task {task_id} requires script verification before completion.\n\n\
                 Attach a passing verification result:\n  \
                 canopy evidence add --task-id {task_id} --source-kind script_verification \\\n    \
                 --source-ref <ref> --label verification --summary 'script verification passed'\n\n\
                 Or override (operators only):\n  \
                 canopy task complete {task_id} --agent-id <agent> --summary '<summary>' --force"
            ));
        }
    }

    // Gate: check for open child tasks (unless --force is used)
    if !force {
        let open_children = match store.list_open_child_tasks(task_id) {
            Ok(children) => children,
            Err(e) => return ToolResult::error(format!("failed to check child tasks: {e}")),
        };
        if !open_children.is_empty() {
            let mut child_list = String::new();
            for (child_id, child_title, child_status) in &open_children {
                let _ = writeln!(child_list, "  {child_id}  {child_title}  [{child_status}]");
            }
            return ToolResult::error(format!(
                "task {task_id} has {} open sub-task(s).\n\n\
                 Complete or cancel all sub-tasks first, or use --force to override.\n\n\
                 Open sub-tasks:\n{}\n\
                 To override:\n  \
                 canopy task complete {task_id} --agent-id <agent> --summary '<summary>' --force",
                open_children.len(),
                child_list
            ));
        }
    }

    let update = TaskStatusUpdate {
        closure_summary: Some(summary),
        ..TaskStatusUpdate::default()
    };

    let task = match store.update_task_status(task_id, TaskStatus::Completed, agent_id, update) {
        Ok(t) => t,
        Err(e) => return ToolResult::error(format!("failed to complete task: {e}")),
    };

    // Attach summary as manual note evidence
    let _ = store.add_evidence(
        task_id,
        crate::models::EvidenceSourceKind::ManualNote,
        task_id,
        "completion_summary",
        Some(summary),
        EvidenceLinkRefs::default(),
    );

    // Log force override if applicable
    if force && task_record.verification_required {
        let _ = store.add_evidence(
            task_id,
            crate::models::EvidenceSourceKind::ManualNote,
            task_id,
            "verification_override",
            Some("completion allowed with --force override despite missing verification"),
            EvidenceLinkRefs::default(),
        );
    }

    if force {
        if let Ok(open_children) = store.list_open_child_tasks(task_id) {
            if !open_children.is_empty() {
                let _ = store.add_evidence(
                    task_id,
                    crate::models::EvidenceSourceKind::ManualNote,
                    task_id,
                    "children_override",
                    Some("completion allowed with --force override despite open sub-tasks"),
                    EvidenceLinkRefs::default(),
                );
            }
        }
    }

    ToolResult::json(&task)
}

/// Mark task as blocked.
pub fn tool_task_block(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let blocked_on = match validate_required_string(args, "blocked_on") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let blocking_task_id = get_str(args, "blocking_task_id");

    let update = TaskStatusUpdate {
        blocked_reason: Some(blocked_on),
        event_note: Some(blocked_on),
        ..TaskStatusUpdate::default()
    };

    let task = match store.update_task_status(task_id, TaskStatus::Blocked, agent_id, update) {
        Ok(t) => t,
        Err(e) => return ToolResult::error(format!("failed to block task: {e}")),
    };

    // If a blocking task is provided, create a dependency relationship
    if let Some(blocking_id) = blocking_task_id {
        let _ = store.apply_task_operator_action(
            task_id,
            agent_id,
            TaskAction::LinkDependency {
                related_task_id: blocking_id,
                relationship_role: TaskRelationshipRole::BlockedBy,
            },
        );
    }

    ToolResult::json(&task)
}

/// Operator dashboard snapshot view.
pub fn tool_task_snapshot(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let preset = get_str(args, "preset").and_then(|s| SnapshotPreset::from_str(s).ok());
    let project_root = get_str(args, "project_root");
    let priority_at_least = get_str(args, "priority").and_then(|s| TaskPriority::from_str(s).ok());
    let severity_at_least = get_str(args, "severity").and_then(|s| TaskSeverity::from_str(s).ok());

    let options = SnapshotOptions {
        project_root,
        preset,
        priority_at_least,
        severity_at_least,
        ..SnapshotOptions::default()
    };

    match api::snapshot(store, options) {
        Ok(snapshot) => ToolResult::json(&snapshot),
        Err(e) => ToolResult::error(format!("failed to build snapshot: {e}")),
    }
}
