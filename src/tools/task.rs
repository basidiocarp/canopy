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

#[cfg(test)]
use crate::store::compute_body_hash;

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
    let workspace = get_str(args, "workspace").map(ToOwned::to_owned);

    let options = TaskCreationOptions {
        required_role,
        required_capabilities,
        verification_required,
        workflow_id,
        phase_id,
        workspace,
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
            workspace: None,
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
            prior_task_id: None,
            immutable_once_dispatched: true,
            body_hash: None,
            branch_of: None,
            branch_at: None,
            branch_outcome: None,
            score: None,
            score_reasons: Vec::new(),
            contract_path: None,
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

    #[test]
    fn completion_signal_conforms_to_contract_with_defaults() {
        let signal = build_completion_signal("task-1", "agent-1", "did the thing", &json!({}));
        // Required fields of canopy-task-completion-signal-v1.
        assert_eq!(signal["schema_version"], "1.0");
        assert_eq!(signal["task_id"], "task-1");
        assert_eq!(signal["agent_id"], "agent-1");
        assert_eq!(signal["status"], "completed");
        assert_eq!(signal["summary"], "did the thing");
        // should_continue is optional; absent when the agent does not supply it
        // (the contract treats absence as "stop"), so it must not be emitted.
        assert!(signal.get("should_continue").is_none());
        // next_action is omitted when not provided.
        assert!(signal.get("next_action").is_none());
    }

    #[test]
    fn completion_signal_carries_should_continue_and_next_action() {
        let args = json!({
            "should_continue": true,
            "next_action": { "follow_up_task_id": "task-2", "directive": "dispatch consumer" }
        });
        let signal = build_completion_signal("task-1", "agent-1", "done", &args);
        assert_eq!(signal["should_continue"], true);
        assert_eq!(signal["next_action"]["follow_up_task_id"], "task-2");
        assert_eq!(signal["next_action"]["directive"], "dispatch consumer");
    }

    #[test]
    fn completion_signal_emits_explicit_should_continue_false() {
        let args = json!({ "should_continue": false });
        let signal = build_completion_signal("task-1", "agent-1", "done", &args);
        // An explicit false is preserved (distinct from absence, though both mean stop).
        assert_eq!(signal["should_continue"], false);
    }

    #[test]
    fn completion_signal_strips_unknown_next_action_keys() {
        let args = json!({
            "should_continue": true,
            "next_action": {
                "follow_up_task_id": "task-2",
                "directive": "go",
                "injected": "should-be-dropped"
            }
        });
        let signal = build_completion_signal("task-1", "agent-1", "done", &args);
        // Only the two contract-allowed keys survive (additionalProperties: false).
        assert_eq!(signal["next_action"]["follow_up_task_id"], "task-2");
        assert_eq!(signal["next_action"]["directive"], "go");
        assert!(signal["next_action"].get("injected").is_none());
    }

    #[test]
    fn completion_signal_drops_wrong_typed_next_action_values() {
        let args = json!({
            "next_action": { "follow_up_task_id": 123, "directive": false }
        });
        let signal = build_completion_signal("task-1", "agent-1", "done", &args);
        // Wrong-typed inner values are dropped, leaving an empty (contract-valid) object.
        assert!(signal["next_action"].get("follow_up_task_id").is_none());
        assert!(signal["next_action"].get("directive").is_none());
    }

    #[test]
    fn completion_signal_allows_null_follow_up_task_id() {
        let args = json!({ "next_action": { "follow_up_task_id": null, "directive": "go" } });
        let signal = build_completion_signal("task-1", "agent-1", "done", &args);
        // null is contract-valid for follow_up_task_id and is preserved.
        assert!(signal["next_action"]["follow_up_task_id"].is_null());
        assert_eq!(signal["next_action"]["directive"], "go");
    }

    #[test]
    fn completion_signal_ignores_non_object_next_action() {
        let args = json!({ "should_continue": true, "next_action": "not-an-object" });
        let signal = build_completion_signal("task-1", "agent-1", "done", &args);
        // A malformed next_action is dropped rather than emitted off-contract.
        assert!(signal.get("next_action").is_none());
        // should_continue still propagates alongside the dropped next_action.
        assert_eq!(signal["should_continue"], true);
    }

    /// Structurally validate the emitted signal against the septa schema:
    /// every required field present, no key outside the schema's `properties`,
    /// and `status` inside the declared enum. Mirrors `tests/contract_alignment.rs`
    /// (no jsonschema crate; skip gracefully when septa is not checked out).
    #[test]
    fn completion_signal_conforms_to_septa_schema() {
        let schema_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("canopy should be inside basidiocarp workspace")
            .join("septa")
            .join("canopy-task-completion-signal-v1.schema.json");
        let Ok(schema_text) = std::fs::read_to_string(&schema_path) else {
            eprintln!("Skipping: {} not found", schema_path.display());
            return;
        };
        let schema: Value = serde_json::from_str(&schema_text).expect("schema must parse");

        // Exercise the richest path so optional fields are present too.
        let args = json!({
            "should_continue": true,
            "next_action": { "follow_up_task_id": "task-2", "directive": "go" }
        });
        let signal = build_completion_signal("task-1", "agent-1", "done", &args);
        let obj = signal.as_object().expect("signal must be an object");

        // (a) every required field is present
        for req in schema["required"].as_array().expect("required array") {
            let name = req.as_str().expect("required entry is a string");
            assert!(obj.contains_key(name), "missing required field {name}");
        }

        // (b) top-level additionalProperties:false → no key outside properties
        let props = schema["properties"].as_object().expect("properties object");
        for key in obj.keys() {
            assert!(
                props.contains_key(key),
                "signal emits off-contract key {key}"
            );
        }

        // (c) status is inside the declared enum
        let status_enum = extract_str_set(&schema["properties"]["status"]["enum"]);
        let status = signal["status"].as_str().expect("status is a string");
        assert!(
            status_enum.contains(status),
            "status {status} not in schema enum {status_enum:?}"
        );

        // (d) next_action carries only its two contract-allowed keys
        let na = signal["next_action"]
            .as_object()
            .expect("next_action object");
        for key in na.keys() {
            assert!(
                ["follow_up_task_id", "directive"].contains(&key.as_str()),
                "next_action emits off-contract key {key}"
            );
        }
    }

    fn extract_str_set(value: &Value) -> std::collections::HashSet<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn test_compute_body_hash_deterministic() {
        let title = "Test task";
        let description = Some("Test description");
        let scope = vec!["file1.rs".to_string(), "file2.rs".to_string()];

        let hash1 = compute_body_hash(title, description, &scope);
        let hash2 = compute_body_hash(title, description, &scope);

        assert_eq!(hash1, hash2, "Body hash should be deterministic");
    }

    #[test]
    fn test_compute_body_hash_changes_on_title_change() {
        let description = Some("Test description");
        let scope = vec!["file1.rs".to_string()];

        let hash1 = compute_body_hash("Original title", description, &scope);
        let hash2 = compute_body_hash("Modified title", description, &scope);

        assert_ne!(hash1, hash2, "Body hash should change when title changes");
    }

    #[test]
    fn test_compute_body_hash_changes_on_description_change() {
        let title = "Test task";
        let scope = vec!["file1.rs".to_string()];

        let hash1 = compute_body_hash(title, Some("Original description"), &scope);
        let hash2 = compute_body_hash(title, Some("Modified description"), &scope);

        assert_ne!(
            hash1, hash2,
            "Body hash should change when description changes"
        );
    }

    #[test]
    fn test_compute_body_hash_changes_on_scope_change() {
        let title = "Test task";
        let description = Some("Test description");

        let hash1 = compute_body_hash(title, description, &["file1.rs".to_string()]);
        let hash2 = compute_body_hash(
            title,
            description,
            &["file1.rs".to_string(), "file2.rs".to_string()],
        );

        assert_ne!(hash1, hash2, "Body hash should change when scope changes");
    }

    #[test]
    fn test_compute_body_hash_handles_empty_description() {
        let title = "Test task";
        let scope = vec!["file1.rs".to_string()];

        let hash1 = compute_body_hash(title, None, &scope);
        let hash2 = compute_body_hash(title, Some(""), &scope);

        assert_eq!(
            hash1, hash2,
            "Body hash should treat None and empty string as equivalent"
        );
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

    let residual_work_warning = match check_handoff_completeness(task_id, args) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let force = get_bool(args, "force").unwrap_or(false);
    let task_record = match load_task_for_completion(store, task_id) {
        Ok(t) => t,
        Err(e) => return e,
    };

    if let Err(e) = validate_completion_gates(store, task_id, &task_record, force) {
        return e;
    }

    let task = match complete_task(store, task_id, agent_id, summary) {
        Ok(t) => t,
        Err(e) => return e,
    };

    if let Err(e) = record_completion_evidence(
        store,
        task_id,
        summary,
        &residual_work_warning,
        &task_record,
        force,
    ) {
        tracing::warn!("failed to record completion evidence: {e:?}");
    }

    persist_task_output(store, task_id, args);

    // Emit the canopy-task-completion-signal-v1 payload alongside the task. The
    // signal rides as a sibling `completion_signal` key so existing top-level
    // task fields stay put (additive); TaskStatus and ToolResult are unchanged.
    let signal = build_completion_signal(task_id, agent_id, summary, args);
    match serde_json::to_value(&task) {
        Ok(Value::Object(mut body)) => {
            body.insert("completion_signal".to_string(), signal);
            ToolResult::json(&Value::Object(body))
        }
        // Task always serializes to an object; fall back to the bare task if not.
        _ => ToolResult::json(&task),
    }
}

/// Build the `canopy-task-completion-signal-v1` payload emitted on completion.
///
/// `should_continue` is the agent's stated intent; it is emitted only when the
/// agent supplies it, matching the contract's optional semantic (absent → stop).
/// `next_action` is an optional follow-on hint reconstructed from only the two
/// contract-allowed keys so a caller cannot inject fields that would violate the
/// schema's `additionalProperties: false`. This tool always reports
/// `status: "completed"` since it is the terminal-completion path.
fn build_completion_signal(task_id: &str, agent_id: &str, summary: &str, args: &Value) -> Value {
    let mut signal = serde_json::json!({
        "schema_version": "1.0",
        "task_id": task_id,
        "agent_id": agent_id,
        "status": "completed",
        "summary": summary,
    });
    if let Some(should_continue) = get_bool(args, "should_continue") {
        signal["should_continue"] = Value::Bool(should_continue);
    }
    if let Some(next_action) = args.get("next_action").and_then(|v| v.as_object()) {
        // Allowlist the two contract-defined keys; drop anything else so the
        // emitted next_action honors the schema's additionalProperties: false.
        let mut allowed = serde_json::Map::new();
        // Type-gate as well as key-gate: follow_up_task_id is string|null and
        // directive is string in the contract. Drop wrong-typed values so a
        // caller cannot emit a schema-violating signal.
        if let Some(id) = next_action
            .get("follow_up_task_id")
            .filter(|v| v.is_string() || v.is_null())
        {
            allowed.insert("follow_up_task_id".to_string(), id.clone());
        }
        if let Some(directive) = next_action.get("directive").filter(|v| v.is_string()) {
            allowed.insert("directive".to_string(), directive.clone());
        }
        signal["next_action"] = Value::Object(allowed);
    }
    signal
}

fn check_handoff_completeness(
    _task_id: &str,
    args: &Value,
) -> std::result::Result<Option<String>, ToolResult> {
    let mut residual_work_warning: Option<String> = None;
    if let Some(handoff_path_str) = get_str(args, "handoff_path") {
        let handoff_path = std::path::Path::new(handoff_path_str);
        match crate::handoff_check::check_completeness(handoff_path) {
            Ok(report) => {
                if !report.is_complete {
                    return Err(ToolResult::error(format!(
                        "completion rejected: {}",
                        crate::handoff_check::format_incomplete_report(&report)
                    )));
                }
                residual_work_warning = report.residual_work_warning;
            }
            Err(e) => {
                return Err(ToolResult::error(format!(
                    "failed to check handoff completeness: {e}"
                )));
            }
        }
    }
    Ok(residual_work_warning)
}

fn load_task_for_completion(
    store: &(impl CanopyStore + ?Sized),
    task_id: &str,
) -> std::result::Result<crate::models::Task, ToolResult> {
    store
        .get_task(task_id)
        .map_err(|e| ToolResult::error(format!("failed to load task: {e}")))
}

fn validate_completion_gates(
    store: &(impl CanopyStore + ?Sized),
    task_id: &str,
    task_record: &crate::models::Task,
    force: bool,
) -> std::result::Result<(), ToolResult> {
    if task_record.verification_required && !force {
        check_verification_evidence(store, task_id)?;
    }
    if !force {
        check_for_open_children(store, task_id)?;
    }
    Ok(())
}

fn check_verification_evidence(
    store: &(impl CanopyStore + ?Sized),
    task_id: &str,
) -> std::result::Result<(), ToolResult> {
    let evidence: Vec<_> = store.list_evidence(task_id).unwrap_or_default();
    let has_passing_verification = evidence.iter().any(|e| match e.source_kind {
        crate::models::EvidenceSourceKind::ScriptVerification => e
            .summary
            .as_deref()
            .is_some_and(|s| s.contains("script verification passed")),
        crate::models::EvidenceSourceKind::RhizomeImpact
        | crate::models::EvidenceSourceKind::CortinaEvent => true,
        _ => false,
    });
    if !has_passing_verification {
        return Err(ToolResult::error(format!(
            "task {task_id} requires verification evidence before completion.\n\n\
             Attach one of:\n  \
             canopy evidence add --task-id {task_id} --source-kind script_verification \\\n    \
             --source-ref <ref> --label verification --summary 'script verification passed'\n  \
             canopy evidence add --task-id {task_id} --source-kind rhizome_impact \\\n    \
             --source-ref <ref> --label verification\n  \
             canopy evidence add --task-id {task_id} --source-kind cortina_event \\\n    \
             --source-ref <ref> --label verification\n\n\
             Or override (operators only):\n  \
             canopy task complete {task_id} --agent-id <agent> --summary '<summary>' --force"
        )));
    }
    Ok(())
}

fn check_for_open_children(
    store: &(impl CanopyStore + ?Sized),
    task_id: &str,
) -> std::result::Result<(), ToolResult> {
    let open_children = store
        .list_open_child_tasks(task_id)
        .map_err(|e| ToolResult::error(format!("failed to check child tasks: {e}")))?;
    if !open_children.is_empty() {
        let mut child_list = String::new();
        for (child_id, child_title, child_status) in &open_children {
            let _ = writeln!(child_list, "  {child_id}  {child_title}  [{child_status}]");
        }
        return Err(ToolResult::error(format!(
            "task {task_id} has {} open sub-task(s).\n\n\
             Complete or cancel all sub-tasks first, or use --force to override.\n\n\
             Open sub-tasks:\n{}\n\
             To override:\n  \
             canopy task complete {task_id} --agent-id <agent> --summary '<summary>' --force",
            open_children.len(),
            child_list
        )));
    }
    Ok(())
}

fn complete_task(
    store: &(impl CanopyStore + ?Sized),
    task_id: &str,
    agent_id: &str,
    summary: &str,
) -> std::result::Result<crate::models::Task, ToolResult> {
    let update = TaskStatusUpdate {
        closure_summary: Some(summary),
        ..TaskStatusUpdate::default()
    };
    store
        .update_task_status(task_id, TaskStatus::Completed, agent_id, update)
        .map_err(|e| ToolResult::error(format!("failed to complete task: {e}")))
}

fn record_completion_evidence(
    store: &(impl CanopyStore + ?Sized),
    task_id: &str,
    summary: &str,
    residual_work_warning: &Option<String>,
    task_record: &crate::models::Task,
    force: bool,
) -> std::result::Result<(), ToolResult> {
    if let Some(warning) = residual_work_warning {
        tracing::warn!(task_id = %task_id, "{warning}");
        if let Err(e) = store.add_evidence(
            task_id,
            crate::models::EvidenceSourceKind::ManualNote,
            task_id,
            "residual_work_warning",
            Some(warning.as_str()),
            EvidenceLinkRefs::default(),
        ) {
            tracing::warn!("failed to record residual_work_warning evidence: {e}");
        }
    }

    if let Err(e) = store.add_evidence(
        task_id,
        crate::models::EvidenceSourceKind::ManualNote,
        task_id,
        "completion_summary",
        Some(summary),
        EvidenceLinkRefs::default(),
    ) {
        tracing::warn!("failed to record completion_summary evidence: {e}");
    }

    if force && task_record.verification_required {
        if let Err(e) = store.add_evidence(
            task_id,
            crate::models::EvidenceSourceKind::ManualNote,
            task_id,
            "verification_override",
            Some("completion allowed with --force override despite missing verification"),
            EvidenceLinkRefs::default(),
        ) {
            tracing::warn!("failed to record verification_override evidence: {e}");
        }
    }

    if force {
        if let Ok(open_children) = store.list_open_child_tasks(task_id) {
            if !open_children.is_empty() {
                if let Err(e) = store.add_evidence(
                    task_id,
                    crate::models::EvidenceSourceKind::ManualNote,
                    task_id,
                    "children_override",
                    Some("completion allowed with --force override despite open sub-tasks"),
                    EvidenceLinkRefs::default(),
                ) {
                    tracing::warn!("failed to record children_override evidence: {e}");
                }
            }
        }
    }

    Ok(())
}

fn persist_task_output(store: &(impl CanopyStore + ?Sized), task_id: &str, args: &Value) {
    if let Some(output_value) = args.get("output") {
        if let Ok(output_json) = serde_json::to_string(output_value) {
            let _ = store.set_task_output(task_id, &output_json);
        }
    }
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

/// Retrieve structured output from a completed task.
pub fn tool_task_output(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match store.get_task_output(task_id) {
        Ok(Some(output_json)) => match serde_json::from_str::<serde_json::Value>(&output_json) {
            Ok(parsed) => ToolResult::json(&parsed),
            Err(e) => ToolResult::error(format!("failed to parse output JSON: {e}")),
        },
        Ok(None) => ToolResult::json(&serde_json::Value::Null),
        Err(e) => ToolResult::error(format!("failed to retrieve task output: {e}")),
    }
}
