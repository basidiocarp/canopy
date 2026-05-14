//! Unix-socket endpoint for direct JSON-RPC 2.0 queries.
//!
//! Cap and other local clients use this endpoint to query canopy coordination data
//! without spawning a subprocess. Bind path is
//! `~/.local/share/basidiocarp/canopy/canopy.sock`. The endpoint
//! descriptor at `~/.config/canopy/canopy.endpoint.json` lets clients
//! discover the socket path via the `local-service-endpoint-v1` convention.
//!
//! # Supported methods
//!
//! - `PING` / `ping` — health probe, returns `{}`
//! - `canopy_snapshot` — full coordination snapshot with optional filters
//! - `canopy_task` — task detail for a specific task
//! - `canopy_agents` — list agents, optionally filtered by `project_root`
//! - `canopy_task_action` — apply an operator action to a task
//! - `canopy_handoff_action` — apply an operator action to a handoff

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Value, json};
use tracing::{debug, error};

const CAPABILITY_ID: &str = "coordination.read.v1";
const PING_METHOD: &str = "PING";

fn write_endpoint_descriptor(socket_path: &Path) -> Result<()> {
    let config_dir = spore::paths::config_dir("canopy");
    std::fs::create_dir_all(&config_dir)?;
    let descriptor_path = config_dir.join("canopy.endpoint.json");
    let descriptor = json!({
        "schema_version": "1.0",
        "transport": "unix-socket",
        "endpoint": socket_path.to_string_lossy(),
        "capability_id": CAPABILITY_ID,
        "version": env!("CARGO_PKG_VERSION"),
        "health_probe": { "method": PING_METHOD, "timeout_ms": 1000 }
    });
    std::fs::write(&descriptor_path, serde_json::to_string_pretty(&descriptor)?)?;
    Ok(())
}

fn remove_stale_socket(socket_path: &Path) {
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC helpers
// ---------------------------------------------------------------------------

fn ok_response(id: &Value, result: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err_response(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() }
    })
}

fn write_response(writer: &mut (impl Write + ?Sized), response: &Value) {
    if let Ok(bytes) = serde_json::to_vec(response) {
        let _ = writer.write_all(&bytes);
        let _ = writer.write_all(b"\n");
        let _ = writer.flush();
    }
}

// ---------------------------------------------------------------------------
// canopy_snapshot handler
// ---------------------------------------------------------------------------

fn handle_snapshot(params: &Value) -> Value {
    use canopy::api::{self, SnapshotOptions};
    use canopy::models::{
        AttentionLevel, SnapshotPreset, TaskPriority, TaskSeverity, TaskSort, TaskView,
    };

    #[derive(serde::Deserialize, Default)]
    struct SnapshotParams {
        project_root: Option<String>,
        preset: Option<SnapshotPreset>,
        sort: Option<TaskSort>,
        view: Option<TaskView>,
        priority_at_least: Option<TaskPriority>,
        severity_at_least: Option<TaskSeverity>,
        acknowledged: Option<bool>,
        attention_at_least: Option<AttentionLevel>,
    }

    let params: SnapshotParams = match serde_json::from_value(params.clone()) {
        Ok(p) => p,
        Err(e) => return json!({ "error": format!("param parse: {e}") }),
    };

    let store = match crate::db::open(None) {
        Ok(s) => s,
        Err(e) => return json!({ "error": format!("store open: {e}") }),
    };

    let options = SnapshotOptions {
        project_root: params.project_root.as_deref(),
        preset: params.preset,
        sort: params.sort,
        view: params.view,
        priority_at_least: params.priority_at_least,
        severity_at_least: params.severity_at_least,
        acknowledged: params.acknowledged,
        attention_at_least: params.attention_at_least,
    };

    match api::snapshot(&store, options) {
        Ok(snapshot) => serde_json::to_value(&snapshot).unwrap_or_else(|_| json!({})),
        Err(e) => json!({ "error": format!("snapshot: {e}") }),
    }
}

// ---------------------------------------------------------------------------
// canopy_task handler
// ---------------------------------------------------------------------------

fn handle_task(params: &Value) -> Value {
    use canopy::api;

    #[derive(serde::Deserialize)]
    struct TaskParams {
        task_id: String,
    }

    let params: TaskParams = match serde_json::from_value(params.clone()) {
        Ok(p) => p,
        Err(e) => return json!({ "error": format!("param parse: {e}") }),
    };

    let store = match crate::db::open(None) {
        Ok(s) => s,
        Err(e) => return json!({ "error": format!("store open: {e}") }),
    };

    match api::task_detail(&store, &params.task_id) {
        Ok(detail) => {
            let wire: canopy::models::TaskDetailWire = detail.into();
            serde_json::to_value(&wire).unwrap_or_else(|_| json!({}))
        }
        Err(e) => json!({ "error": format!("task detail: {e}") }),
    }
}

// ---------------------------------------------------------------------------
// canopy_agents handler
// ---------------------------------------------------------------------------

fn handle_agents(params: &Value) -> Value {
    #[derive(serde::Deserialize, Default)]
    struct AgentsParams {
        project_root: Option<String>,
    }

    let params: AgentsParams = match serde_json::from_value(params.clone()) {
        Ok(p) => p,
        Err(e) => return json!({ "error": format!("param parse: {e}") }),
    };

    let store = match crate::db::open(None) {
        Ok(s) => s,
        Err(e) => return json!({ "error": format!("store open: {e}") }),
    };

    let agents = if let Some(project_root) = params.project_root.as_deref() {
        match store.list_agents_filtered(Some(project_root)) {
            Ok(a) => a,
            Err(e) => return json!({ "error": format!("list agents: {e}") }),
        }
    } else {
        match store.list_agents() {
            Ok(a) => a,
            Err(e) => return json!({ "error": format!("list agents: {e}") }),
        }
    };

    match serde_json::to_value(&agents) {
        Ok(v) => v,
        Err(_) => json!({}),
    }
}

// ---------------------------------------------------------------------------
// canopy_task_action handler
// ---------------------------------------------------------------------------

fn handle_task_action(params: &Value) -> Value {
    use canopy::models::{OperatorActionKind, TaskAction};

    #[derive(serde::Deserialize)]
    struct TaskActionParams {
        task_id: String,
        action: String,
        acting_agent_id: Option<String>,
        author_agent_id: Option<String>,
        assigned_to: Option<String>,
        blocked_reason: Option<String>,
        changed_by: String,
        clear_owner_note: Option<bool>,
        closure_summary: Option<String>,
        due_at: Option<String>,
        review_due_at: Option<String>,
        evidence_label: Option<String>,
        evidence_source_kind: Option<String>,
        evidence_source_ref: Option<String>,
        evidence_summary: Option<String>,
        expires_at: Option<String>,
        follow_up_title: Option<String>,
        follow_up_description: Option<String>,
        force: Option<bool>,
        from_agent_id: Option<String>,
        handoff_summary: Option<String>,
        handoff_type: Option<String>,
        message_body: Option<String>,
        message_type: Option<String>,
        note: Option<String>,
        owner_note: Option<String>,
        priority: Option<String>,
        related_file: Option<String>,
        related_handoff_id: Option<String>,
        related_memory_query: Option<String>,
        related_session_id: Option<String>,
        related_symbol: Option<String>,
        related_task_id: Option<String>,
        relationship_role: Option<String>,
        requested_action: Option<String>,
        review_annotation_action: Option<String>,
        review_annotation_anchor_hash: Option<String>,
        review_annotation_comment: Option<String>,
        review_annotation_end_line: Option<i64>,
        review_annotation_file_path: Option<String>,
        review_annotation_start_line: Option<i64>,
        severity: Option<String>,
        to_agent_id: Option<String>,
        verification_state: Option<String>,
    }

    let params: TaskActionParams = match serde_json::from_value(params.clone()) {
        Ok(p) => p,
        Err(e) => return json!({ "error": format!("param parse: {e}") }),
    };

    let store = match crate::db::open(None) {
        Ok(s) => s,
        Err(e) => return json!({ "error": format!("store open: {e}") }),
    };

    // Parse action string to OperatorActionKind
    let action_kind: OperatorActionKind = match params.action.as_str() {
        "acknowledge_task" => OperatorActionKind::AcknowledgeTask,
        "unacknowledge_task" => OperatorActionKind::UnacknowledgeTask,
        "set_task_priority" => OperatorActionKind::SetTaskPriority,
        "set_task_severity" => OperatorActionKind::SetTaskSeverity,
        "update_task_note" => OperatorActionKind::UpdateTaskNote,
        "set_task_due_at" => OperatorActionKind::SetTaskDueAt,
        "clear_task_due_at" => OperatorActionKind::ClearTaskDueAt,
        "set_review_due_at" => OperatorActionKind::SetReviewDueAt,
        "clear_review_due_at" => OperatorActionKind::ClearReviewDueAt,
        "verify_task" => OperatorActionKind::VerifyTask,
        "close_task" => OperatorActionKind::CloseTask,
        "block_task" => OperatorActionKind::BlockTask,
        "unblock_task" => OperatorActionKind::UnblockTask,
        "reopen_blocked_task_when_unblocked" => OperatorActionKind::ReopenBlockedTaskWhenUnblocked,
        "claim_task" => OperatorActionKind::ClaimTask,
        "start_task" => OperatorActionKind::StartTask,
        "resume_task" => OperatorActionKind::ResumeTask,
        "pause_task" => OperatorActionKind::PauseTask,
        "yield_task" => OperatorActionKind::YieldTask,
        "complete_task" => OperatorActionKind::CompleteTask,
        "reassign_task" => OperatorActionKind::ReassignTask,
        "record_decision" => OperatorActionKind::RecordDecision,
        "create_handoff" => OperatorActionKind::CreateHandoff,
        "summon_council_session" => OperatorActionKind::SummonCouncilSession,
        "post_council_message" => OperatorActionKind::PostCouncilMessage,
        "attach_evidence" => OperatorActionKind::AttachEvidence,
        "attach_review_annotation" => OperatorActionKind::AttachReviewAnnotation,
        "create_follow_up_task" => OperatorActionKind::CreateFollowUpTask,
        "link_task_dependency" => OperatorActionKind::LinkTaskDependency,
        "resolve_dependency" => OperatorActionKind::ResolveDependency,
        "promote_follow_up" => OperatorActionKind::PromoteFollowUp,
        _ => return json!({ "error": format!("unknown action: {}", params.action) }),
    };

    // Parse enum fields
    let priority = params
        .priority
        .as_deref()
        .and_then(|p| serde_json::from_value(serde_json::json!(p)).ok());
    let severity = params
        .severity
        .as_deref()
        .and_then(|s| serde_json::from_value(serde_json::json!(s)).ok());
    let verification_state = params
        .verification_state
        .as_deref()
        .and_then(|v| serde_json::from_value(serde_json::json!(v)).ok());
    let message_type = params
        .message_type
        .as_deref()
        .and_then(|m| serde_json::from_value(serde_json::json!(m)).ok());
    let evidence_source_kind = params
        .evidence_source_kind
        .as_deref()
        .and_then(|e| serde_json::from_value(serde_json::json!(e)).ok());
    let handoff_type = params
        .handoff_type
        .as_deref()
        .and_then(|h| serde_json::from_value(serde_json::json!(h)).ok());
    let relationship_role = params
        .relationship_role
        .as_deref()
        .and_then(|r| serde_json::from_value(serde_json::json!(r)).ok());
    let review_annotation_action = params
        .review_annotation_action
        .as_deref()
        .and_then(|a| serde_json::from_value(serde_json::json!(a)).ok());

    // Build TaskAction using the pattern from app.rs
    let task_action = match action_kind {
        canopy::models::OperatorActionKind::AcknowledgeTask => TaskAction::Acknowledge {
            note: params.note.as_deref(),
        },
        canopy::models::OperatorActionKind::UnacknowledgeTask => TaskAction::Unacknowledge {
            note: params.note.as_deref(),
        },
        canopy::models::OperatorActionKind::SetTaskPriority => {
            if priority.is_none() {
                return json!({ "error": "set_task_priority requires priority" });
            }
            TaskAction::SetPriority {
                priority: priority.unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::SetTaskSeverity => {
            if severity.is_none() {
                return json!({ "error": "set_task_severity requires severity" });
            }
            TaskAction::SetSeverity {
                severity: severity.unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::UpdateTaskNote => TaskAction::UpdateNote {
            owner_note: params.owner_note.as_deref(),
            clear_owner_note: params.clear_owner_note.unwrap_or(false),
            note: params.note.as_deref(),
        },
        canopy::models::OperatorActionKind::SetTaskDueAt => {
            if params.due_at.is_none() {
                return json!({ "error": "set_task_due_at requires due_at" });
            }
            TaskAction::SetDueAt {
                due_at: params.due_at.as_deref().unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::ClearTaskDueAt => TaskAction::ClearDueAt {
            note: params.note.as_deref(),
        },
        canopy::models::OperatorActionKind::SetReviewDueAt => {
            if params.review_due_at.is_none() {
                return json!({ "error": "set_review_due_at requires review_due_at" });
            }
            TaskAction::SetReviewDueAt {
                review_due_at: params.review_due_at.as_deref().unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::ClearReviewDueAt => TaskAction::ClearReviewDueAt {
            note: params.note.as_deref(),
        },
        canopy::models::OperatorActionKind::VerifyTask => {
            if verification_state.is_none() {
                return json!({ "error": "verify_task requires verification_state" });
            }
            TaskAction::Verify {
                verification_state: verification_state.unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::CloseTask => {
            if params.closure_summary.is_none() {
                return json!({ "error": "close_task requires closure_summary" });
            }
            TaskAction::Close {
                closure_summary: params.closure_summary.as_deref().unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::BlockTask => {
            if params.blocked_reason.is_none() {
                return json!({ "error": "block_task requires blocked_reason" });
            }
            TaskAction::Block {
                blocked_reason: params.blocked_reason.as_deref().unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::UnblockTask => TaskAction::Unblock {
            note: params.note.as_deref(),
        },
        canopy::models::OperatorActionKind::ReopenBlockedTaskWhenUnblocked => {
            TaskAction::ReopenWhenUnblocked {
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::ClaimTask => {
            if params.acting_agent_id.is_none() {
                return json!({ "error": "claim_task requires acting_agent_id" });
            }
            TaskAction::Claim {
                acting_agent_id: params.acting_agent_id.as_deref().unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::StartTask => {
            if params.acting_agent_id.is_none() {
                return json!({ "error": "start_task requires acting_agent_id" });
            }
            TaskAction::Start {
                acting_agent_id: params.acting_agent_id.as_deref().unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::ResumeTask => {
            if params.acting_agent_id.is_none() {
                return json!({ "error": "resume_task requires acting_agent_id" });
            }
            TaskAction::Resume {
                acting_agent_id: params.acting_agent_id.as_deref().unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::PauseTask => {
            if params.acting_agent_id.is_none() {
                return json!({ "error": "pause_task requires acting_agent_id" });
            }
            TaskAction::Pause {
                acting_agent_id: params.acting_agent_id.as_deref().unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::YieldTask => {
            if params.acting_agent_id.is_none() {
                return json!({ "error": "yield_task requires acting_agent_id" });
            }
            TaskAction::Yield {
                acting_agent_id: params.acting_agent_id.as_deref().unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::CompleteTask => {
            if params.acting_agent_id.is_none() {
                return json!({ "error": "complete_task requires acting_agent_id" });
            }
            TaskAction::Complete {
                acting_agent_id: params.acting_agent_id.as_deref().unwrap(),
                note: params.note.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::ReassignTask => {
            if params.assigned_to.is_none() {
                return json!({ "error": "reassign_task requires assigned_to" });
            }
            TaskAction::Reassign {
                assigned_to: params.assigned_to.as_deref().unwrap(),
                note: params.note.as_deref(),
                force: params.force.unwrap_or(false),
            }
        }
        canopy::models::OperatorActionKind::RecordDecision => {
            if params.author_agent_id.is_none() {
                return json!({ "error": "record_decision requires author_agent_id" });
            }
            if params.message_body.is_none() {
                return json!({ "error": "record_decision requires message_body" });
            }
            TaskAction::RecordDecision {
                author_agent_id: params.author_agent_id.as_deref().unwrap(),
                message_body: params.message_body.as_deref().unwrap(),
            }
        }
        canopy::models::OperatorActionKind::CreateHandoff => {
            if params.from_agent_id.is_none() {
                return json!({ "error": "create_handoff requires from_agent_id" });
            }
            if params.to_agent_id.is_none() {
                return json!({ "error": "create_handoff requires to_agent_id" });
            }
            if handoff_type.is_none() {
                return json!({ "error": "create_handoff requires handoff_type" });
            }
            if params.handoff_summary.is_none() {
                return json!({ "error": "create_handoff requires handoff_summary" });
            }
            TaskAction::CreateHandoff {
                from_agent_id: params.from_agent_id.as_deref().unwrap(),
                to_agent_id: params.to_agent_id.as_deref().unwrap(),
                handoff_type: handoff_type.unwrap(),
                handoff_summary: params.handoff_summary.as_deref().unwrap(),
                requested_action: params.requested_action.as_deref(),
                due_at: params.due_at.as_deref(),
                expires_at: params.expires_at.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::SummonCouncilSession => {
            TaskAction::SummonCouncilSession
        }
        canopy::models::OperatorActionKind::PostCouncilMessage => {
            if params.author_agent_id.is_none() {
                return json!({ "error": "post_council_message requires author_agent_id" });
            }
            if message_type.is_none() {
                return json!({ "error": "post_council_message requires message_type" });
            }
            if params.message_body.is_none() {
                return json!({ "error": "post_council_message requires message_body" });
            }
            TaskAction::PostCouncilMessage {
                author_agent_id: params.author_agent_id.as_deref().unwrap(),
                message_type: message_type.unwrap(),
                message_body: params.message_body.as_deref().unwrap(),
            }
        }
        canopy::models::OperatorActionKind::AttachEvidence => {
            if evidence_source_kind.is_none() {
                return json!({ "error": "attach_evidence requires evidence_source_kind" });
            }
            if params.evidence_source_ref.is_none() {
                return json!({ "error": "attach_evidence requires evidence_source_ref" });
            }
            if params.evidence_label.is_none() {
                return json!({ "error": "attach_evidence requires evidence_label" });
            }
            TaskAction::AttachEvidence {
                source_kind: evidence_source_kind.unwrap(),
                source_ref: params.evidence_source_ref.as_deref().unwrap(),
                label: params.evidence_label.as_deref().unwrap(),
                summary: params.evidence_summary.as_deref(),
                related_handoff_id: params.related_handoff_id.as_deref(),
                related_session_id: params.related_session_id.as_deref(),
                related_memory_query: params.related_memory_query.as_deref(),
                related_symbol: params.related_symbol.as_deref(),
                related_file: params.related_file.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::AttachReviewAnnotation => {
            if params.review_annotation_file_path.is_none() {
                return json!({ "error": "attach_review_annotation requires review_annotation_file_path" });
            }
            if params.review_annotation_start_line.is_none()
                || params.review_annotation_end_line.is_none()
            {
                return json!({ "error": "attach_review_annotation requires review_annotation_start_line and review_annotation_end_line" });
            }
            if review_annotation_action.is_none() {
                return json!({ "error": "attach_review_annotation requires review_annotation_action" });
            }
            if params.review_annotation_anchor_hash.is_none() {
                return json!({ "error": "attach_review_annotation requires review_annotation_anchor_hash" });
            }
            if params.review_annotation_comment.is_none() {
                return json!({ "error": "attach_review_annotation requires review_annotation_comment" });
            }
            TaskAction::AttachReviewAnnotation {
                file_path: params.review_annotation_file_path.as_deref().unwrap(),
                start_line: params.review_annotation_start_line.unwrap(),
                end_line: params.review_annotation_end_line.unwrap(),
                action: review_annotation_action.unwrap(),
                comment: params.review_annotation_comment.as_deref().unwrap(),
                anchor_hash: params.review_annotation_anchor_hash.as_deref().unwrap(),
            }
        }
        canopy::models::OperatorActionKind::CreateFollowUpTask => {
            if params.follow_up_title.is_none() {
                return json!({ "error": "create_follow_up_task requires follow_up_title" });
            }
            TaskAction::CreateFollowUp {
                title: params.follow_up_title.as_deref().unwrap(),
                description: params.follow_up_description.as_deref(),
            }
        }
        canopy::models::OperatorActionKind::LinkTaskDependency => {
            if params.related_task_id.is_none() {
                return json!({ "error": "link_task_dependency requires related_task_id" });
            }
            if relationship_role.is_none() {
                return json!({ "error": "link_task_dependency requires relationship_role" });
            }
            TaskAction::LinkDependency {
                related_task_id: params.related_task_id.as_deref().unwrap(),
                relationship_role: relationship_role.unwrap(),
            }
        }
        canopy::models::OperatorActionKind::ResolveDependency => {
            if params.related_task_id.is_none() {
                return json!({ "error": "resolve_dependency requires related_task_id" });
            }
            TaskAction::ResolveDependency {
                related_task_id: params.related_task_id.as_deref().unwrap(),
            }
        }
        canopy::models::OperatorActionKind::PromoteFollowUp => {
            if params.related_task_id.is_none() {
                return json!({ "error": "promote_follow_up requires related_task_id" });
            }
            TaskAction::PromoteFollowUp {
                related_task_id: params.related_task_id.as_deref().unwrap(),
            }
        }
        canopy::models::OperatorActionKind::CloseFollowUpChain => TaskAction::CloseFollowUpChain,
        // Handoff-related actions are not valid for task actions
        canopy::models::OperatorActionKind::AcceptHandoff
        | canopy::models::OperatorActionKind::RejectHandoff
        | canopy::models::OperatorActionKind::CancelHandoff
        | canopy::models::OperatorActionKind::CompleteHandoff
        | canopy::models::OperatorActionKind::FollowUpHandoff
        | canopy::models::OperatorActionKind::ExpireHandoff => {
            return json!({ "error": format!("action {} is only valid for handoffs, not tasks", params.action) });
        }
    };

    match store.apply_task_operator_action(&params.task_id, &params.changed_by, task_action) {
        Ok(task) => serde_json::to_value(&task).unwrap_or_else(|_| json!({})),
        Err(e) => json!({ "error": format!("task action: {e}") }),
    }
}

// ---------------------------------------------------------------------------
// canopy_handoff_action handler
// ---------------------------------------------------------------------------

fn handle_handoff_action(params: &Value) -> Value {
    use canopy::models::OperatorActionKind;
    use canopy::store::HandoffOperatorActionInput;

    #[derive(serde::Deserialize)]
    struct HandoffActionParams {
        handoff_id: String,
        action: String,
        acting_agent_id: Option<String>,
        changed_by: String,
        note: Option<String>,
    }

    let params: HandoffActionParams = match serde_json::from_value(params.clone()) {
        Ok(p) => p,
        Err(e) => return json!({ "error": format!("param parse: {e}") }),
    };

    let store = match crate::db::open(None) {
        Ok(s) => s,
        Err(e) => return json!({ "error": format!("store open: {e}") }),
    };

    // Parse action string to OperatorActionKind
    let action_kind: OperatorActionKind = match params.action.as_str() {
        "accept_handoff" => OperatorActionKind::AcceptHandoff,
        "reject_handoff" => OperatorActionKind::RejectHandoff,
        "cancel_handoff" => OperatorActionKind::CancelHandoff,
        "complete_handoff" => OperatorActionKind::CompleteHandoff,
        "follow_up_handoff" => OperatorActionKind::FollowUpHandoff,
        "expire_handoff" => OperatorActionKind::ExpireHandoff,
        _ => return json!({ "error": format!("unknown handoff action: {}", params.action) }),
    };

    match store.apply_handoff_operator_action(
        &params.handoff_id,
        action_kind,
        &params.changed_by,
        HandoffOperatorActionInput {
            acting_agent_id: params.acting_agent_id.as_deref(),
            note: params.note.as_deref(),
        },
    ) {
        Ok(handoff) => serde_json::to_value(&handoff).unwrap_or_else(|_| json!({})),
        Err(e) => json!({ "error": format!("handoff action: {e}") }),
    }
}

// ---------------------------------------------------------------------------
// Connection handler
// ---------------------------------------------------------------------------

fn handle_connection(stream: std::os::unix::net::UnixStream) {
    let writer_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            error!("failed to clone unix stream: {e}");
            return;
        }
    };
    let mut reader = BufReader::new(stream);
    let mut writer = writer_stream;

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) => {
                error!("socket read error: {e}");
                return;
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let resp = err_response(&Value::Null, -32700, format!("parse error: {e}"));
                write_response(&mut writer, &resp);
                return;
            }
        };

        let id = match msg.get("id").cloned() {
            Some(id) if !id.is_null() => id,
            _ => continue, // notification — no response
        };

        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        debug!("socket request: {method}");

        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

        let response = match method {
            m if m == PING_METHOD || m == "ping" => {
                let empty = json!({});
                ok_response(&id, &empty)
            }
            "canopy_snapshot" => {
                let result = handle_snapshot(&params);
                if result.get("error").is_some() {
                    let msg = result["error"]
                        .as_str()
                        .unwrap_or("snapshot error")
                        .to_string();
                    err_response(&id, -32000, msg)
                } else {
                    ok_response(&id, &result)
                }
            }
            "canopy_task" => {
                let result = handle_task(&params);
                if result.get("error").is_some() {
                    let msg = result["error"].as_str().unwrap_or("task error").to_string();
                    err_response(&id, -32000, msg)
                } else {
                    ok_response(&id, &result)
                }
            }
            "canopy_agents" => {
                let result = handle_agents(&params);
                if result.get("error").is_some() {
                    let msg = result["error"]
                        .as_str()
                        .unwrap_or("agents error")
                        .to_string();
                    err_response(&id, -32000, msg)
                } else {
                    ok_response(&id, &result)
                }
            }
            "canopy_task_action" => {
                let result = handle_task_action(&params);
                if result.get("error").is_some() {
                    let msg = result["error"]
                        .as_str()
                        .unwrap_or("task action error")
                        .to_string();
                    err_response(&id, -32000, msg)
                } else {
                    ok_response(&id, &result)
                }
            }
            "canopy_handoff_action" => {
                let result = handle_handoff_action(&params);
                if result.get("error").is_some() {
                    let msg = result["error"]
                        .as_str()
                        .unwrap_or("handoff action error")
                        .to_string();
                    err_response(&id, -32000, msg)
                } else {
                    ok_response(&id, &result)
                }
            }
            _ => err_response(&id, -32601, format!("method not found: {method}")),
        };

        write_response(&mut writer, &response);
    }
}

// ---------------------------------------------------------------------------
// Server entry point
// ---------------------------------------------------------------------------

/// Start the canopy unix-socket service endpoint.
///
/// Binds to `~/.local/share/basidiocarp/canopy/canopy.sock`, writes the
/// endpoint descriptor to `~/.config/canopy/canopy.endpoint.json`, then
/// accepts connections indefinitely. Each connection is handled in a
/// background thread.
pub fn run_socket_server() -> Result<()> {
    let socket_path: PathBuf = spore::paths::data_dir("basidiocarp")
        .join("canopy")
        .join("canopy.sock");

    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    remove_stale_socket(&socket_path);

    let listener = std::os::unix::net::UnixListener::bind(&socket_path).map_err(|e| {
        anyhow::anyhow!(
            "failed to bind canopy socket {}: {e}",
            socket_path.display()
        )
    })?;

    write_endpoint_descriptor(&socket_path)?;

    tracing::info!(
        socket = %socket_path.display(),
        "canopy socket endpoint ready"
    );

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                std::thread::spawn(move || handle_connection(stream));
            }
            Err(e) => error!("canopy socket accept error: {e}"),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use tempfile::TempDir;

    fn temp_socket_path(dir: &TempDir) -> PathBuf {
        dir.path().join("test.sock")
    }

    #[test]
    fn socket_server_ping_responds_ok() {
        let tmp = TempDir::new().unwrap();
        let socket_path = temp_socket_path(&tmp);

        remove_stale_socket(&socket_path);
        let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        let socket_path_clone = socket_path.clone();

        let handle = std::thread::spawn(move || {
            if let Ok(stream) = listener.accept().map(|(s, _)| s) {
                handle_connection(stream);
            }
        });

        let mut client = std::os::unix::net::UnixStream::connect(&socket_path_clone).unwrap();
        let request = r#"{"jsonrpc":"2.0","id":1,"method":"PING","params":null}"#;
        client.write_all(request.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        client.flush().unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();

        let reader = BufReader::new(&client);
        let line = reader.lines().next().expect("response").unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], 1);
        assert!(v.get("result").is_some());
        assert!(v.get("error").is_none());

        handle.join().unwrap();
    }

    #[test]
    fn socket_server_unknown_method_returns_method_not_found() {
        let tmp = TempDir::new().unwrap();
        let socket_path = temp_socket_path(&tmp);

        remove_stale_socket(&socket_path);
        let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        let socket_path_clone = socket_path.clone();

        let handle = std::thread::spawn(move || {
            if let Ok(stream) = listener.accept().map(|(s, _)| s) {
                handle_connection(stream);
            }
        });

        let mut client = std::os::unix::net::UnixStream::connect(&socket_path_clone).unwrap();
        let request = r#"{"jsonrpc":"2.0","id":2,"method":"no_such_method","params":{}}"#;
        client.write_all(request.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        client.flush().unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();

        let reader = BufReader::new(&client);
        let line = reader.lines().next().expect("response").unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], 2);
        assert!(v.get("error").is_some());
        assert_eq!(v["error"]["code"], -32601);

        handle.join().unwrap();
    }
}
