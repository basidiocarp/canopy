use canopy::api::{self, SnapshotOptions};
use canopy::models::{
    AgentRegistration, AgentRole, AgentStatus, EvidenceSourceKind, HandoffType, SnapshotPreset,
    TaskStatus, VerificationState,
};
use canopy::store::{
    EvidenceLinkRefs, HandoffTiming, Store, TaskCreationOptions, TaskStatusUpdate,
};
use canopy::tools::task::tool_task_snapshot;
use serde_json::{Value, json};
use tempfile::tempdir;

fn register_agent(
    store: &Store,
    agent_id: &str,
    host_id: &str,
    host_type: &str,
    host_instance: &str,
    model: &str,
    project_root: &str,
) {
    let agent = AgentRegistration {
        agent_id: agent_id.to_string(),
        host_id: host_id.to_string(),
        host_type: host_type.to_string(),
        host_instance: host_instance.to_string(),
        model: model.to_string(),
        project_root: project_root.to_string(),
        worktree_id: "wt-1".to_string(),
        role: Some(AgentRole::Implementer),
        capabilities: vec!["rust".to_string()],
        status: AgentStatus::Idle,
        current_task_id: None,
        heartbeat_at: None,
    };

    store.register_agent(&agent).expect("register agent");
}

#[test]
fn task_snapshot_tool_matches_api_snapshot_projection() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");
    let project_root = "/tmp/foundation-alignment";

    register_agent(
        &store,
        "codex-1",
        "codex-local",
        "codex",
        "local",
        "gpt-5.4",
        project_root,
    );
    register_agent(
        &store,
        "claude-1",
        "claude-local",
        "claude",
        "local",
        "opus",
        project_root,
    );

    let task = store
        .create_task_with_options(
            "Foundation alignment",
            Some("Keep coordination state explicit"),
            "operator",
            project_root,
            &TaskCreationOptions::default(),
        )
        .expect("create task");

    store
        .update_task_status(
            &task.task_id,
            TaskStatus::ReviewRequired,
            "operator",
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Pending),
                ..TaskStatusUpdate::default()
            },
        )
        .expect("mark task review required");

    let handoff = store
        .create_handoff(
            &task.task_id,
            "codex-1",
            "claude-1",
            HandoffType::RequestReview,
            "Please review the task boundary",
            Some("review the operator boundary"),
            HandoffTiming::default(),
        )
        .expect("create handoff");

    let evidence = store
        .add_evidence(
            &task.task_id,
            EvidenceSourceKind::ManualNote,
            "note-1",
            "Boundary note",
            Some("Snapshot should surface explicit evidence"),
            EvidenceLinkRefs {
                related_handoff_id: Some(&handoff.handoff_id),
                ..EvidenceLinkRefs::default()
            },
        )
        .expect("attach evidence");

    let options = SnapshotOptions {
        project_root: Some(project_root),
        preset: Some(SnapshotPreset::Attention),
        ..SnapshotOptions::default()
    };
    let api_snapshot = api::snapshot(&store, options).expect("build api snapshot");

    let tool_result = tool_task_snapshot(
        &store,
        "operator",
        &json!({
            "project_root": project_root,
            "preset": "attention"
        }),
    );
    assert!(!tool_result.is_error, "tool snapshot should succeed");
    assert_eq!(tool_result.content.len(), 1);

    let tool_snapshot: Value =
        serde_json::from_str(&tool_result.content[0].text).expect("parse tool snapshot");
    let api_snapshot: Value = serde_json::to_value(api_snapshot).expect("serialize api snapshot");

    assert_eq!(tool_snapshot, api_snapshot);
    assert_eq!(tool_snapshot["evidence"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        tool_snapshot["evidence"][0]["evidence_id"],
        evidence.evidence_id
    );
    assert_eq!(
        tool_snapshot["evidence"][0]["related_handoff_id"],
        handoff.handoff_id
    );
    assert_eq!(tool_snapshot["tasks"][0]["task_id"], task.task_id);
}
