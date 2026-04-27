#![allow(
    clippy::filter_map_bool_then,
    clippy::manual_contains,
    clippy::similar_names,
    clippy::too_many_lines
)]

use assert_cmd::Command;
use canopy::api::{self, SnapshotOptions};
use canopy::models::{
    AgentRegistration, AgentRole, AgentStatus, BreachSeverity, DeadlineState, OperatorActionKind,
    SnapshotPreset, TaskAttentionReason, TaskDeadlineKind, TaskStatus, VerificationState,
};
use canopy::store::{Store, TaskCreationOptions, TaskDeadlineUpdate, TaskStatusUpdate};
use chrono::{Duration, Utc};
use rusqlite::{Connection, params};
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn api_snapshot_includes_agents_tasks_handoffs_and_evidence() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    for (agent_id, host_id, host_type, host_instance, model, project_root) in [
        (
            "codex-1",
            "codex-local",
            "codex",
            "local",
            "gpt-5.4",
            "/tmp/project",
        ),
        (
            "claude-1",
            "claude-local",
            "claude",
            "local",
            "opus",
            "/tmp/project",
        ),
        (
            "codex-2",
            "codex-alt",
            "codex",
            "remote",
            "gpt-5.4-mini",
            "/tmp/other-project",
        ),
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                host_id,
                "--host-type",
                host_type,
                "--host-instance",
                host_instance,
                "--model",
                model,
                "--project-root",
                project_root,
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    let task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Snapshot task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task: Value = serde_json::from_slice(&task_output).expect("parse task");
    let task_id = task["task_id"].as_str().expect("task id").to_string();

    let handoff_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            &task_id,
            "--from-agent-id",
            "codex-1",
            "--to-agent-id",
            "claude-1",
            "--handoff-type",
            "request_review",
            "--summary",
            "ask for review",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let handoff: Value = serde_json::from_slice(&handoff_output).expect("parse handoff");
    let handoff_id = handoff["handoff_id"]
        .as_str()
        .expect("handoff id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "evidence",
            "add",
            "--task-id",
            &task_id,
            "--source-kind",
            "hyphae_session",
            "--source-ref",
            "session:01KMSCANOPY",
            "--label",
            "hyphae session",
            "--related-handoff-id",
            &handoff_id,
            "--related-session-id",
            "ses_123",
        ])
        .assert()
        .success();

    let blocker_task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Repair lifecycle blocker",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let blocker_task: Value =
        serde_json::from_slice(&blocker_task_output).expect("parse blocker task");
    let blocker_task_id = blocker_task["task_id"]
        .as_str()
        .expect("blocker task id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &task_id,
            "--action",
            "create_follow_up_task",
            "--changed-by",
            "operator",
            "--follow-up-title",
            "Track rollout cleanups",
            "--follow-up-description",
            "Capture the remaining operator work",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &task_id,
            "--action",
            "link_task_dependency",
            "--changed-by",
            "operator",
            "--related-task-id",
            &blocker_task_id,
            "--relationship-role",
            "blocked_by",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "assign",
            "--task-id",
            &task_id,
            "--assigned-to",
            "codex-1",
            "--assigned-by",
            "operator",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &task_id,
            "--status",
            "review_required",
            "--changed-by",
            "operator",
            "--verification-state",
            "pending",
        ])
        .assert()
        .success();

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "review",
            "--sort",
            "updated_at",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    assert_eq!(snapshot["attention"]["tasks_needing_attention"], 1);
    assert_eq!(snapshot["attention"]["critical_tasks"], 0);
    assert_eq!(snapshot["attention"]["actionable_tasks"], 1);
    assert_eq!(snapshot["attention"]["actionable_handoffs"], 0);
    assert_eq!(
        snapshot["task_attention"]
            .as_array()
            .expect("task attention")
            .len(),
        1
    );
    assert_eq!(snapshot["task_attention"][0]["level"], "needs_attention");
    assert_eq!(snapshot["task_attention"][0]["freshness"], "fresh");
    assert_eq!(
        snapshot["task_attention"][0]["reasons"]
            .as_array()
            .expect("task attention reasons")
            .iter()
            .map(|value| value.as_str().expect("reason"))
            .collect::<Vec<_>>(),
        vec![
            "review_required",
            "review_with_graph_pressure",
            "review_handoff_follow_through",
            "review_awaiting_support",
            "has_open_follow_ups",
            "awaiting_handoff_acceptance",
            "unacknowledged",
        ]
    );
    assert_eq!(snapshot["agents"].as_array().expect("agents").len(), 2);
    assert_eq!(
        snapshot["agent_attention"]
            .as_array()
            .expect("agent attention")
            .len(),
        2
    );
    assert_eq!(
        snapshot["heartbeats"].as_array().expect("heartbeats").len(),
        2
    );
    assert_eq!(
        snapshot["task_heartbeat_summaries"]
            .as_array()
            .expect("task heartbeat summaries")
            .len(),
        1
    );
    assert_eq!(
        snapshot["task_heartbeat_summaries"][0]["heartbeat_count"],
        2
    );
    assert_eq!(
        snapshot["agent_heartbeat_summaries"]
            .as_array()
            .expect("agent heartbeat summaries")
            .len(),
        2
    );
    assert_eq!(
        snapshot["ownership"].as_array().expect("ownership").len(),
        1
    );
    assert_eq!(snapshot["ownership"][0]["assignment_count"], 1);
    assert_eq!(snapshot["ownership"][0]["last_assigned_to"], "codex-1");
    assert_eq!(snapshot["tasks"].as_array().expect("tasks").len(), 1);
    assert_eq!(snapshot["handoffs"].as_array().expect("handoffs").len(), 1);
    assert_eq!(
        snapshot["operator_actions"]
            .as_array()
            .expect("operator actions")
            .len(),
        5
    );
    assert_eq!(snapshot["operator_actions"][0]["kind"], "acknowledge_task");
    assert_eq!(snapshot["operator_actions"][0]["target_kind"], "task");
    assert_eq!(snapshot["operator_actions"][1]["kind"], "verify_task");
    assert_eq!(snapshot["operator_actions"][1]["target_kind"], "task");
    assert_eq!(
        snapshot["operator_actions"][2]["kind"],
        "summon_council_session"
    );
    assert_eq!(snapshot["operator_actions"][2]["target_kind"], "task");
    assert_eq!(
        snapshot["operator_actions"][3]["kind"],
        "resolve_dependency"
    );
    assert_eq!(snapshot["operator_actions"][3]["target_kind"], "task");
    assert_eq!(snapshot["operator_actions"][4]["kind"], "promote_follow_up");
    assert_eq!(snapshot["operator_actions"][4]["target_kind"], "task");
    assert_eq!(
        snapshot["workflow_contexts"]
            .as_array()
            .expect("workflow contexts")
            .len(),
        1
    );
    // Evidence was attached, so the review cycle advances to in_review.
    // Prior to A35 this was "pending" because EvidenceAttached events were not
    // emitted; the state machine now correctly reflects attached evidence.
    assert_eq!(
        snapshot["workflow_contexts"][0]["review_cycle"]["state"],
        "in_review"
    );
    assert_eq!(snapshot["evidence"].as_array().expect("evidence").len(), 1);
    assert_eq!(
        snapshot["relationships"]
            .as_array()
            .expect("relationships")
            .len(),
        2
    );
    assert_eq!(
        snapshot["relationship_summaries"]
            .as_array()
            .expect("relationship summaries")
            .len(),
        1
    );
    assert_eq!(
        snapshot["relationship_summaries"][0]["open_follow_up_child_count"],
        1
    );
    assert_eq!(snapshot["sla_summary"]["due_soon_count"], 0);
    assert_eq!(snapshot["sla_summary"]["overdue_count"], 0);
    assert_eq!(
        snapshot["sla_summary"]["oldest_overdue_seconds"],
        Value::Null
    );
    assert_eq!(snapshot["sla_summary"]["breach_severity"], "none");
    assert_eq!(
        snapshot["task_sla_summaries"]
            .as_array()
            .expect("task sla summaries")
            .len(),
        1
    );
    assert_eq!(snapshot["task_sla_summaries"][0]["task_id"], task_id);
    assert_eq!(snapshot["task_sla_summaries"][0]["due_soon_count"], 0);
    assert_eq!(snapshot["task_sla_summaries"][0]["overdue_count"], 0);
    assert_eq!(
        snapshot["task_sla_summaries"][0]["highest_risk_queue"],
        Value::Null
    );
    assert_eq!(snapshot["task_sla_summaries"][0]["breach_severity"], "none");

    let task_detail_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "task",
            "--task-id",
            &task_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let detail: Value = serde_json::from_slice(&task_detail_output).expect("parse task detail");
    assert_eq!(detail["task"]["status"], "review_required");
    assert_eq!(detail["task"]["verification_state"], "pending");
    assert_eq!(detail["attention"]["level"], "needs_attention");
    assert_eq!(detail["attention"]["reasons"][0], "review_required");
    let event_types = detail["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|event| event["event_type"].as_str().expect("event type"))
        .collect::<Vec<_>>();
    // EvidenceAttached event is now emitted at the store layer (A35), adding one
    // event to the timeline vs the prior 6.
    assert_eq!(event_types.len(), 7);
    assert!(event_types.contains(&"created"));
    assert!(event_types.contains(&"assigned"));
    assert!(event_types.contains(&"status_changed"));
    assert!(event_types.contains(&"evidence_attached"));
    assert!(event_types.contains(&"follow_up_task_created"));
    assert_eq!(
        event_types
            .iter()
            .filter(|event_type| **event_type == "relationship_updated")
            .count(),
        2
    );
    assert_eq!(
        detail["heartbeats"].as_array().expect("heartbeats").len(),
        2
    );
    assert_eq!(detail["heartbeat_summary"]["heartbeat_count"], 2);
    assert_eq!(
        detail["agent_heartbeat_summaries"]
            .as_array()
            .expect("agent heartbeat summaries")
            .len(),
        2
    );
    assert_eq!(detail["ownership"]["assignment_count"], 1);
    assert_eq!(detail["sla_summary"]["due_soon_count"], 0);
    assert_eq!(detail["sla_summary"]["overdue_count"], 0);
    assert_eq!(detail["sla_summary"]["oldest_overdue_seconds"], Value::Null);
    assert_eq!(detail["sla_summary"]["highest_risk_queue"], Value::Null);
    assert_eq!(detail["sla_summary"]["breach_severity"], "none");
    assert_eq!(
        detail["assignments"].as_array().expect("assignments").len(),
        1
    );
    assert_eq!(detail["handoffs"].as_array().expect("handoffs").len(), 1);
    assert_eq!(
        detail["handoff_attention"]
            .as_array()
            .expect("handoff attention")
            .len(),
        1
    );
    assert_eq!(
        detail["operator_actions"]
            .as_array()
            .expect("operator actions")
            .len(),
        5
    );
    assert_eq!(
        detail["workflow_context"]["queue_state"]["status"],
        "review"
    );
    let allowed_actions = detail["allowed_actions"]
        .as_array()
        .expect("allowed actions");
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "acknowledge_task")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "verify_task")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "reassign_task")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "set_task_priority")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "set_task_severity")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "update_task_note")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "create_handoff")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "summon_council_session")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "attach_evidence")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "create_follow_up_task")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "link_task_dependency")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "resolve_dependency")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "promote_follow_up")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "block_task")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "follow_up_handoff")
    );
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "expire_handoff")
    );
    assert_eq!(detail["evidence"].as_array().expect("evidence").len(), 1);
    assert_eq!(detail["evidence"][0]["related_session_id"], "ses_123");
    assert!(detail["evidence"][0]["related_memory_query"].is_null());
    assert_eq!(
        detail["relationships"]
            .as_array()
            .expect("relationships")
            .len(),
        2
    );
    assert_eq!(detail["relationship_summary"]["blocker_count"], 1);
    assert_eq!(
        detail["relationship_summary"]["open_follow_up_child_count"],
        1
    );
    assert_eq!(
        detail["related_tasks"]
            .as_array()
            .expect("related tasks")
            .len(),
        2
    );
    assert!(
        detail["related_tasks"]
            .as_array()
            .expect("related tasks")
            .iter()
            .any(|related| related["relationship_role"] == "follow_up_child")
    );
    assert!(
        detail["related_tasks"]
            .as_array()
            .expect("related tasks")
            .iter()
            .any(|related| related["relationship_role"] == "blocked_by")
    );
}

#[test]
fn api_snapshot_includes_agent_capabilities_and_task_required_capabilities() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    store
        .register_agent(&AgentRegistration {
            agent_id: "codex-1".to_string(),
            host_id: "codex-local".to_string(),
            host_type: "codex".to_string(),
            host_instance: "local".to_string(),
            model: "gpt-5.4".to_string(),
            project_root: "/tmp/project".to_string(),
            worktree_id: "wt-1".to_string(),
            role: Some(AgentRole::Implementer),
            capabilities: vec!["rust".to_string(), "hyphae".to_string()],
            status: AgentStatus::Idle,
            current_task_id: None,
            heartbeat_at: None,
        })
        .expect("register agent");

    let task = store
        .create_task_with_options(
            "Capability snapshot task",
            None,
            "operator",
            "/tmp/project",
            &TaskCreationOptions {
                required_role: Some(AgentRole::Implementer),
                required_capabilities: vec!["rust".to_string(), "hyphae".to_string()],
                auto_review: false,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create task");

    let snapshot = api::snapshot(
        &store,
        SnapshotOptions {
            project_root: Some("/tmp/project"),
            ..SnapshotOptions::default()
        },
    )
    .expect("load snapshot");
    assert_eq!(snapshot.agents[0].capabilities, vec!["rust", "hyphae"]);
    assert_eq!(
        snapshot.tasks[0].required_capabilities,
        vec!["rust", "hyphae"]
    );

    let detail = api::task_detail(&store, &task.task_id).expect("load task detail");
    assert_eq!(
        detail.task.required_capabilities,
        vec!["rust".to_string(), "hyphae".to_string()]
    );
}

#[test]
fn api_task_detail_exposes_handoff_resolution_actions_for_open_handoffs() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    for (agent_id, host_id, host_type, host_instance, model) in [
        ("codex-1", "codex-local", "codex", "local", "gpt-5.4"),
        ("claude-1", "claude-local", "claude", "local", "opus"),
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                host_id,
                "--host-type",
                host_type,
                "--host-instance",
                host_instance,
                "--model",
                model,
                "--project-root",
                "/tmp/project",
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    let task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Resolve open handoff",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task: Value = serde_json::from_slice(&task_output).expect("parse task");
    let task_id = task["task_id"].as_str().expect("task id").to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            &task_id,
            "--from-agent-id",
            "codex-1",
            "--to-agent-id",
            "claude-1",
            "--handoff-type",
            "request_review",
            "--summary",
            "review this before close",
        ])
        .assert()
        .success();

    let detail_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "task",
            "--task-id",
            &task_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let detail: Value = serde_json::from_slice(&detail_output).expect("parse detail");
    let kinds = detail["allowed_actions"]
        .as_array()
        .expect("allowed actions")
        .iter()
        .filter_map(|action| {
            (action["target_kind"] == "handoff").then(|| action["kind"].as_str().expect("kind"))
        })
        .collect::<Vec<_>>();

    assert!(kinds.contains(&"accept_handoff"));
    assert!(kinds.contains(&"reject_handoff"));
    assert!(kinds.contains(&"cancel_handoff"));
    assert!(kinds.contains(&"complete_handoff"));
    assert!(kinds.contains(&"follow_up_handoff"));
    assert!(kinds.contains(&"expire_handoff"));
}

#[test]
fn api_snapshot_evidence_add_uses_env_runtime_session_id_when_flag_is_omitted() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    let task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Snapshot task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task: Value = serde_json::from_slice(&task_output).expect("parse task");
    let task_id = task["task_id"].as_str().expect("task id").to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .env("CLAUDE_SESSION_ID", "claude-session-42")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "evidence",
            "add",
            "--task-id",
            &task_id,
            "--source-kind",
            "hyphae_session",
            "--source-ref",
            "session:01KMSCANOPY",
            "--label",
            "hyphae session",
        ])
        .assert()
        .success();

    let detail_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "task",
            "--task-id",
            &task_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let detail: Value = serde_json::from_slice(&detail_output).expect("parse task detail");
    let evidence = detail["evidence"].as_array().expect("evidence");
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0]["related_session_id"], "claude-session-42");
}

#[test]
fn api_task_detail_limits_expired_open_handoffs_to_expire_action() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    for (agent_id, host_id, host_type, host_instance, model) in [
        ("codex-1", "codex-local", "codex", "local", "gpt-5.4"),
        ("claude-1", "claude-local", "claude", "local", "opus"),
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                host_id,
                "--host-type",
                host_type,
                "--host-instance",
                host_instance,
                "--model",
                model,
                "--project-root",
                "/tmp/project",
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    let task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Expire open handoff",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task: Value = serde_json::from_slice(&task_output).expect("parse task");
    let task_id = task["task_id"].as_str().expect("task id").to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            &task_id,
            "--from-agent-id",
            "codex-1",
            "--to-agent-id",
            "claude-1",
            "--handoff-type",
            "request_review",
            "--summary",
            "review this before close",
            "--expires-at",
            "2020-01-01T00:00:00Z",
        ])
        .assert()
        .success();

    let detail_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "task",
            "--task-id",
            &task_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let detail: Value = serde_json::from_slice(&detail_output).expect("parse detail");
    let kinds = detail["allowed_actions"]
        .as_array()
        .expect("allowed actions")
        .iter()
        .filter_map(|action| {
            (action["target_kind"] == "handoff").then(|| action["kind"].as_str().expect("kind"))
        })
        .collect::<Vec<_>>();

    assert_eq!(kinds, vec!["expire_handoff"]);
}

#[test]
fn api_snapshot_status_sort_uses_operator_priority_order() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    for (agent_id, host_id, host_type, host_instance, model) in [
        ("codex-1", "codex-local", "codex", "local", "gpt-5.4"),
        ("claude-1", "claude-local", "claude", "local", "opus"),
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                host_id,
                "--host-type",
                host_type,
                "--host-instance",
                host_instance,
                "--model",
                model,
                "--project-root",
                "/tmp/project",
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    for (title, status) in [
        ("Blocked task", "blocked"),
        ("Review task", "review_required"),
        ("Active task", "in_progress"),
    ] {
        let task_output = Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "task",
                "create",
                "--title",
                title,
                "--requested-by",
                "operator",
                "--project-root",
                "/tmp/project",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let task: Value = serde_json::from_slice(&task_output).expect("parse task");
        let task_id = task["task_id"].as_str().expect("task id").to_string();

        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "task",
                "status",
                "--task-id",
                &task_id,
                "--status",
                status,
                "--changed-by",
                "operator",
                "--blocked-reason",
                if status == "blocked" { "waiting" } else { "" },
            ])
            .assert()
            .success();
    }

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--sort",
            "status",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let titles: Vec<_> = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["title"].as_str().expect("title"))
        .collect();
    assert_eq!(titles, vec!["Active task", "Review task", "Blocked task"]);
}

#[test]
fn api_snapshot_attention_view_returns_only_tasks_needing_attention() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    for (agent_id, host_id, host_type, host_instance, model) in [
        ("codex-1", "codex-local", "codex", "local", "gpt-5.4"),
        ("claude-1", "claude-local", "claude", "local", "opus"),
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                host_id,
                "--host-type",
                host_type,
                "--host-instance",
                host_instance,
                "--model",
                model,
                "--project-root",
                "/tmp/project",
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    let healthy_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Healthy task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let healthy_task: Value = serde_json::from_slice(&healthy_output).expect("parse task");

    let blocked_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Blocked task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let blocked_task: Value = serde_json::from_slice(&blocked_output).expect("parse task");
    let blocked_task_id = blocked_task["task_id"].as_str().expect("blocked task id");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            blocked_task_id,
            "--status",
            "blocked",
            "--changed-by",
            "operator",
            "--blocked-reason",
            "waiting on review",
        ])
        .assert()
        .success();

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "attention",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let task_titles: Vec<_> = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["title"].as_str().expect("title"))
        .collect();
    assert_eq!(task_titles, vec!["Blocked task"]);
    assert_eq!(snapshot["attention"]["tasks_needing_attention"], 1);
    assert_eq!(snapshot["task_attention"][0]["level"], "critical");
    assert_eq!(snapshot["task_attention"][0]["task_id"], blocked_task_id);
    assert_ne!(
        snapshot["task_attention"][0]["task_id"],
        healthy_task["task_id"]
    );
}

#[test]
fn api_snapshot_uses_attention_thresholds_for_stale_task_handoff_and_owner_heartbeat() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    for (agent_id, host_id, host_type, host_instance, model) in [
        ("codex-1", "codex-local", "codex", "local", "gpt-5.4"),
        ("claude-1", "claude-local", "claude", "local", "opus"),
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                host_id,
                "--host-type",
                host_type,
                "--host-instance",
                host_instance,
                "--model",
                model,
                "--project-root",
                "/tmp/project",
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    let task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Stale task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task: Value = serde_json::from_slice(&task_output).expect("parse task");
    let task_id = task["task_id"].as_str().expect("task id").to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "assign",
            "--task-id",
            &task_id,
            "--assigned-to",
            "codex-1",
            "--assigned-by",
            "operator",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            &task_id,
            "--from-agent-id",
            "codex-1",
            "--to-agent-id",
            "claude-1",
            "--handoff-type",
            "request_help",
            "--summary",
            "Needs another pass",
        ])
        .assert()
        .success();

    let conn = Connection::open(&db_path).expect("open db");
    conn.execute(
        "UPDATE tasks SET updated_at = ?1 WHERE task_id = ?2",
        params!["2026-03-01 00:00:00", task_id],
    )
    .expect("age task");
    conn.execute(
        "UPDATE agents SET heartbeat_at = ?1 WHERE agent_id = 'codex-1'",
        params!["2026-03-01 00:00:00"],
    )
    .expect("age heartbeat");
    conn.execute(
        "UPDATE handoffs SET created_at = ?1, updated_at = ?1 WHERE task_id = ?2",
        params!["2026-03-01 00:00:00", task_id],
    )
    .expect("age handoff");

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "attention",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let reasons: Vec<_> = snapshot["task_attention"][0]["reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .map(|value| value.as_str().expect("reason"))
        .collect();
    assert_eq!(snapshot["task_attention"][0]["level"], "critical");
    assert_eq!(snapshot["task_attention"][0]["freshness"], "stale");
    assert_eq!(
        snapshot["task_attention"][0]["owner_heartbeat_freshness"],
        "stale"
    );
    assert_eq!(
        snapshot["task_attention"][0]["open_handoff_freshness"],
        "stale"
    );
    assert!(reasons.contains(&"stale_update"));
    assert!(reasons.contains(&"stale_owner_heartbeat"));
    assert!(reasons.contains(&"stale_open_handoff"));
    assert_eq!(snapshot["handoff_attention"][0]["level"], "critical");
    let stale_agent = snapshot["agent_attention"]
        .as_array()
        .expect("agent attention")
        .iter()
        .find(|item| item["agent_id"] == "codex-1")
        .expect("stale codex agent");
    assert_eq!(stale_agent["freshness"], "stale");
    assert_eq!(snapshot["attention"]["critical_tasks"], 1);
    assert_eq!(snapshot["attention"]["stale_handoffs"], 1);
    assert_eq!(snapshot["attention"]["stale_agents"], 1);
}

#[test]
fn api_snapshot_presets_and_triage_filters_use_runtime_metadata() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    let urgent_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Urgent task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let urgent_task: Value = serde_json::from_slice(&urgent_output).expect("parse task");
    let urgent_task_id = urgent_task["task_id"]
        .as_str()
        .expect("task id")
        .to_string();

    let normal_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Normal task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let normal_task: Value = serde_json::from_slice(&normal_output).expect("parse normal task");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "triage",
            "--task-id",
            &urgent_task_id,
            "--changed-by",
            "operator",
            "--priority",
            "high",
            "--severity",
            "critical",
            "--acknowledged",
            "false",
            "--owner-note",
            "escalate now",
        ])
        .assert()
        .success();

    let critical_snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "critical",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let critical_snapshot: Value =
        serde_json::from_slice(&critical_snapshot_output).expect("parse critical snapshot");

    let critical_task_ids: Vec<_> = critical_snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(critical_task_ids, vec![urgent_task_id.as_str()]);
    assert_eq!(critical_snapshot["task_attention"][0]["level"], "critical");
    assert_eq!(
        critical_snapshot["task_attention"][0]["acknowledged"],
        false
    );

    let severity_snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--severity-at-least",
            "critical",
            "--sort",
            "severity",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let severity_snapshot: Value =
        serde_json::from_slice(&severity_snapshot_output).expect("parse severity snapshot");
    assert_eq!(
        severity_snapshot["tasks"].as_array().expect("tasks").len(),
        1
    );
    assert_eq!(severity_snapshot["tasks"][0]["task_id"], urgent_task_id);
    assert_ne!(
        severity_snapshot["tasks"][0]["task_id"],
        normal_task["task_id"]
    );

    let blocker_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Blocker task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let blocker_task: Value = serde_json::from_slice(&blocker_output).expect("parse blocker task");
    let blocker_task_id = blocker_task["task_id"]
        .as_str()
        .expect("blocker task id")
        .to_string();

    let blocked_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Dependency blocked task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let blocked_task: Value = serde_json::from_slice(&blocked_output).expect("parse blocked task");
    let blocked_task_id = blocked_task["task_id"]
        .as_str()
        .expect("blocked task id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &blocked_task_id,
            "--status",
            "blocked",
            "--changed-by",
            "operator",
            "--blocked-reason",
            "waiting on blocker task",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &blocked_task_id,
            "--action",
            "link_task_dependency",
            "--changed-by",
            "operator",
            "--related-task-id",
            &blocker_task_id,
            "--relationship-role",
            "blocked_by",
        ])
        .assert()
        .success();

    let follow_up_parent_id = normal_task["task_id"]
        .as_str()
        .expect("normal task id")
        .to_string();
    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &follow_up_parent_id,
            "--action",
            "create_follow_up_task",
            "--changed-by",
            "operator",
            "--follow-up-title",
            "Normal follow-up",
        ])
        .assert()
        .success();

    let dependency_snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "blocked_by_dependencies",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let dependency_snapshot: Value =
        serde_json::from_slice(&dependency_snapshot_output).expect("parse dependency snapshot");
    let dependency_task_ids: Vec<_> = dependency_snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(dependency_task_ids, vec![blocked_task_id.as_str()]);
    assert_eq!(
        dependency_snapshot["relationship_summaries"][0]["active_blocker_count"],
        1
    );
    assert!(
        dependency_snapshot["task_attention"][0]["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .any(|reason| reason == "blocked_by_active_dependency")
    );

    let follow_up_snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "follow_up_chains",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let follow_up_snapshot: Value =
        serde_json::from_slice(&follow_up_snapshot_output).expect("parse follow-up snapshot");
    assert_eq!(
        follow_up_snapshot["tasks"].as_array().expect("tasks").len(),
        2
    );
    assert!(
        follow_up_snapshot["relationship_summaries"]
            .as_array()
            .expect("relationship summaries")
            .iter()
            .any(|summary| summary["open_follow_up_child_count"] == 1)
    );
}

#[test]
fn api_snapshot_updated_at_sort_handles_mixed_timestamp_formats() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    for title in ["Older task", "Newer task"] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "task",
                "create",
                "--title",
                title,
                "--requested-by",
                "operator",
                "--project-root",
                "/tmp/project",
            ])
            .assert()
            .success();
    }

    let conn = Connection::open(&db_path).expect("open db");
    conn.execute(
        "UPDATE tasks SET updated_at = '2026-03-27 10:00:00' WHERE title = 'Older task'",
        [],
    )
    .expect("update sqlite timestamp");
    conn.execute(
        "UPDATE tasks SET updated_at = '2026-03-28T10:00:00Z' WHERE title = 'Newer task'",
        [],
    )
    .expect("update rfc3339 timestamp");

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--sort",
            "updated_at",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let titles: Vec<_> = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["title"].as_str().expect("title"))
        .collect();
    assert_eq!(titles, vec!["Newer task", "Older task"]);
}

#[test]
fn api_snapshot_review_with_graph_pressure_view_tracks_review_tasks_with_open_relationship_pressure()
 {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "agent",
            "register",
            "--agent-id",
            "agent-a",
            "--host-id",
            "codex-local",
            "--host-type",
            "codex",
            "--host-instance",
            "local",
            "--model",
            "gpt-5.4",
            "--project-root",
            "/tmp/project",
            "--worktree-id",
            "wt-1",
        ])
        .assert()
        .success();

    let review_task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Review pressure task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let review_task: Value = serde_json::from_slice(&review_task_output).expect("parse task");
    let review_task_id = review_task["task_id"]
        .as_str()
        .expect("task id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "assign",
            "--task-id",
            &review_task_id,
            "--assigned-to",
            "agent-a",
            "--assigned-by",
            "operator",
            "--reason",
            "review owner",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &review_task_id,
            "--status",
            "review_required",
            "--changed-by",
            "operator",
            "--verification-state",
            "pending",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &review_task_id,
            "--action",
            "create_follow_up_task",
            "--changed-by",
            "operator",
            "--follow-up-title",
            "Follow-up pressure child",
        ])
        .assert()
        .success();

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_with_graph_pressure",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let task_ids: Vec<_> = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(task_ids, vec![review_task_id.as_str()]);
    assert!(
        snapshot["task_attention"][0]["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .any(|reason| reason == "review_with_graph_pressure")
    );
    assert_eq!(
        snapshot["relationship_summaries"][0]["open_follow_up_child_count"],
        1
    );

    let default_snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let default_snapshot: Value =
        serde_json::from_slice(&default_snapshot_output).expect("parse default snapshot");
    assert!(
        default_snapshot["task_attention"]
            .as_array()
            .expect("task attention")
            .iter()
            .any(|attention| {
                attention["task_id"] == review_task_id
                    && attention["reasons"]
                        .as_array()
                        .expect("reasons")
                        .iter()
                        .any(|reason| reason == "review_with_graph_pressure")
            })
    );
}

#[test]
fn api_snapshot_review_handoff_follow_through_tracks_open_and_accepted_review_handoffs() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    for agent_id in [
        "agent-a", "agent-b", "agent-c", "agent-d", "agent-e", "agent-f",
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                "codex-local",
                "--host-type",
                "codex",
                "--host-instance",
                "local",
                "--model",
                "gpt-5.4",
                "--project-root",
                "/tmp/project",
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    let review_task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Review handoff task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let review_task: Value = serde_json::from_slice(&review_task_output).expect("parse task");
    let review_task_id = review_task["task_id"]
        .as_str()
        .expect("task id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &review_task_id,
            "--status",
            "review_required",
            "--changed-by",
            "operator",
            "--verification-state",
            "pending",
        ])
        .assert()
        .success();

    let handoff_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            &review_task_id,
            "--from-agent-id",
            "agent-a",
            "--to-agent-id",
            "agent-b",
            "--handoff-type",
            "request_review",
            "--summary",
            "review this task before closeout",
            "--expires-at",
            "2099-01-01T00:00:00Z",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let handoff: Value = serde_json::from_slice(&handoff_output).expect("parse handoff");
    let handoff_id = handoff["handoff_id"]
        .as_str()
        .expect("handoff id")
        .to_string();

    let open_snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_handoff_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let open_snapshot: Value =
        serde_json::from_slice(&open_snapshot_output).expect("parse open snapshot");
    let open_task_ids: Vec<_> = open_snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(open_task_ids, vec![review_task_id.as_str()]);
    assert!(
        open_snapshot["task_attention"][0]["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .any(|reason| reason == "review_handoff_follow_through")
    );

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "resolve",
            "--handoff-id",
            &handoff_id,
            "--status",
            "accepted",
            "--resolved-by",
            "agent-b",
            "--acting-agent-id",
            "agent-b",
        ])
        .assert()
        .success();

    let accepted_snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_handoff_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let accepted_snapshot: Value =
        serde_json::from_slice(&accepted_snapshot_output).expect("parse accepted snapshot");
    let accepted_task_ids: Vec<_> = accepted_snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(accepted_task_ids, vec![review_task_id.as_str()]);
    assert!(
        accepted_snapshot["task_attention"][0]["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .any(|reason| reason == "review_handoff_follow_through")
    );
}

#[test]
fn api_snapshot_review_handoff_follow_through_splits_due_soon_and_overdue() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let now = Utc::now();
    let due_soon_review_due_at = (now + Duration::hours(1)).to_rfc3339();
    let overdue_review_due_at = (now - Duration::hours(1)).to_rfc3339();
    let review_handoff_expires_at = (now + Duration::hours(2)).to_rfc3339();

    for agent_id in ["agent-a", "agent-b"] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                "codex-local",
                "--host-type",
                "codex",
                "--host-instance",
                "local",
                "--model",
                "gpt-5.4",
                "--project-root",
                "/tmp/project",
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    for title in [
        "Due soon review follow-through",
        "Overdue review follow-through",
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "task",
                "create",
                "--title",
                title,
                "--requested-by",
                "operator",
                "--project-root",
                "/tmp/project",
            ])
            .assert()
            .success();
    }

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let due_soon_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Due soon review follow-through")
        .and_then(|task| task["task_id"].as_str())
        .expect("due soon task id")
        .to_string();
    let overdue_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Overdue review follow-through")
        .and_then(|task| task["task_id"].as_str())
        .expect("overdue task id")
        .to_string();

    for task_id in [&due_soon_task_id, &overdue_task_id] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "task",
                "status",
                "--task-id",
                task_id,
                "--status",
                "review_required",
                "--changed-by",
                "operator",
                "--verification-state",
                "pending",
            ])
            .assert()
            .success();
    }

    let create_review_handoff = |task_id: &str, due_at: &str| {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "handoff",
                "create",
                "--task-id",
                task_id,
                "--from-agent-id",
                "agent-a",
                "--to-agent-id",
                "agent-b",
                "--handoff-type",
                "request_review",
                "--summary",
                "review before closeout",
                "--due-at",
                due_at,
                "--expires-at",
                review_handoff_expires_at.as_str(),
            ])
            .assert()
            .success();
    };

    create_review_handoff(&due_soon_task_id, due_soon_review_due_at.as_str());
    create_review_handoff(&overdue_task_id, overdue_review_due_at.as_str());

    let due_soon_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "due_soon_review_handoff_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let due_soon_snapshot: Value =
        serde_json::from_slice(&due_soon_output).expect("parse due soon review snapshot");
    let due_soon_task_ids: Vec<_> = due_soon_snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(due_soon_task_ids, vec![due_soon_task_id.as_str()]);

    let overdue_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "overdue_review_handoff_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let overdue_snapshot: Value =
        serde_json::from_slice(&overdue_output).expect("parse overdue review snapshot");
    let overdue_task_ids: Vec<_> = overdue_snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(overdue_task_ids, vec![overdue_task_id.as_str()]);
}

#[test]
fn api_snapshot_review_decision_follow_through_tracks_open_decision_and_closeout_handoffs() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    for agent_id in [
        "agent-a", "agent-b", "agent-c", "agent-d", "agent-e", "agent-f",
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                agent_id,
                "--host-type",
                "codex",
                "--host-instance",
                "local",
                "--model",
                "gpt-5.4",
                "--project-root",
                "/tmp/project",
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    let review_task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Decision follow-through task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let review_task: Value = serde_json::from_slice(&review_task_output).expect("parse task");
    let review_task_id = review_task["task_id"]
        .as_str()
        .expect("task id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &review_task_id,
            "--status",
            "review_required",
            "--changed-by",
            "operator",
            "--verification-state",
            "pending",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &review_task_id,
            "--action",
            "attach_evidence",
            "--changed-by",
            "operator",
            "--evidence-source-kind",
            "manual_note",
            "--evidence-source-ref",
            "decision-input-1",
            "--evidence-label",
            "Decision input",
        ])
        .assert()
        .success();

    let decision_handoff_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            &review_task_id,
            "--from-agent-id",
            "agent-a",
            "--to-agent-id",
            "agent-b",
            "--handoff-type",
            "record_decision",
            "--summary",
            "Need final decision before closeout",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let decision_handoff: Value =
        serde_json::from_slice(&decision_handoff_output).expect("parse handoff");
    let decision_handoff_id = decision_handoff["handoff_id"]
        .as_str()
        .expect("handoff id")
        .to_string();

    let decision_snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_decision_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let decision_snapshot: Value =
        serde_json::from_slice(&decision_snapshot_output).expect("parse snapshot");
    let decision_task_ids: Vec<_> = decision_snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(decision_task_ids, vec![review_task_id.as_str()]);
    assert!(
        decision_snapshot["task_attention"][0]["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .any(|reason| reason == "review_decision_follow_through")
    );

    let ready_for_closeout_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_ready_for_closeout",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let ready_for_closeout_snapshot: Value =
        serde_json::from_slice(&ready_for_closeout_output).expect("parse snapshot");
    assert!(
        ready_for_closeout_snapshot["tasks"]
            .as_array()
            .expect("tasks")
            .is_empty()
    );

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "resolve",
            "--handoff-id",
            &decision_handoff_id,
            "--status",
            "accepted",
            "--resolved-by",
            "agent-b",
            "--acting-agent-id",
            "agent-b",
        ])
        .assert()
        .success();

    let accepted_snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_decision_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let accepted_snapshot: Value =
        serde_json::from_slice(&accepted_snapshot_output).expect("parse accepted snapshot");
    assert_eq!(
        accepted_snapshot["tasks"]
            .as_array()
            .expect("tasks")
            .iter()
            .map(|task| task["task_id"].as_str().expect("task id"))
            .collect::<Vec<_>>(),
        vec![review_task_id.as_str()]
    );

    let closeout_task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Closeout follow-through task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let closeout_task: Value =
        serde_json::from_slice(&closeout_task_output).expect("parse closeout task");
    let closeout_task_id = closeout_task["task_id"]
        .as_str()
        .expect("closeout task id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &closeout_task_id,
            "--status",
            "review_required",
            "--changed-by",
            "operator",
            "--verification-state",
            "pending",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &closeout_task_id,
            "--action",
            "attach_evidence",
            "--changed-by",
            "operator",
            "--evidence-source-kind",
            "manual_note",
            "--evidence-source-ref",
            "closeout-input-1",
            "--evidence-label",
            "Closeout input",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            &closeout_task_id,
            "--from-agent-id",
            "agent-a",
            "--to-agent-id",
            "agent-b",
            "--handoff-type",
            "close_task",
            "--summary",
            "Need final closeout handoff before closing",
        ])
        .assert()
        .success();

    let mixed_snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_decision_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mixed_snapshot: Value =
        serde_json::from_slice(&mixed_snapshot_output).expect("parse mixed snapshot");
    let mut mixed_task_ids: Vec<_> = mixed_snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    mixed_task_ids.sort_unstable();
    let mut expected_task_ids = vec![closeout_task_id.as_str(), review_task_id.as_str()];
    expected_task_ids.sort_unstable();
    assert_eq!(mixed_task_ids, expected_task_ids);

    let mixed_attention = mixed_snapshot["task_attention"]
        .as_array()
        .expect("task attention");
    assert!(mixed_attention.iter().any(|attention| {
        attention["task_id"] == closeout_task_id
            && attention["reasons"]
                .as_array()
                .expect("reasons")
                .iter()
                .any(|reason| reason == "review_decision_follow_through")
    }));

    let still_not_closeout_ready_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_ready_for_closeout",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let still_not_closeout_ready: Value = serde_json::from_slice(&still_not_closeout_ready_output)
        .expect("parse closeout-ready snapshot");
    assert!(
        still_not_closeout_ready["tasks"]
            .as_array()
            .expect("tasks")
            .is_empty()
    );
}

#[test]
fn api_snapshot_review_decision_follow_through_splits_due_soon_and_overdue() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let now = Utc::now();
    let due_soon_due_at = (now + Duration::hours(1)).to_rfc3339();
    let overdue_due_at = (now - Duration::hours(1)).to_rfc3339();
    let decision_handoff_expires_at = (now + Duration::hours(2)).to_rfc3339();

    for agent_id in ["agent-a", "agent-b"] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                "codex-local",
                "--host-type",
                "codex",
                "--host-instance",
                "local",
                "--model",
                "gpt-5.4",
                "--project-root",
                "/tmp/project",
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    for title in [
        "Due soon decision follow-through",
        "Overdue decision follow-through",
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "task",
                "create",
                "--title",
                title,
                "--requested-by",
                "operator",
                "--project-root",
                "/tmp/project",
            ])
            .assert()
            .success();
    }

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let due_soon_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Due soon decision follow-through")
        .and_then(|task| task["task_id"].as_str())
        .expect("due soon task id")
        .to_string();
    let overdue_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Overdue decision follow-through")
        .and_then(|task| task["task_id"].as_str())
        .expect("overdue task id")
        .to_string();

    for task_id in [&due_soon_task_id, &overdue_task_id] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "task",
                "status",
                "--task-id",
                task_id,
                "--status",
                "review_required",
                "--changed-by",
                "operator",
                "--verification-state",
                "pending",
            ])
            .assert()
            .success();
    }

    let create_decision_handoff = |task_id: &str, due_at: &str| {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "handoff",
                "create",
                "--task-id",
                task_id,
                "--from-agent-id",
                "agent-a",
                "--to-agent-id",
                "agent-b",
                "--handoff-type",
                "record_decision",
                "--summary",
                "decision before closeout",
                "--due-at",
                due_at,
                "--expires-at",
                decision_handoff_expires_at.as_str(),
            ])
            .assert()
            .success();
    };

    create_decision_handoff(&due_soon_task_id, due_soon_due_at.as_str());
    create_decision_handoff(&overdue_task_id, overdue_due_at.as_str());

    let due_soon_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "due_soon_review_decision_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let due_soon_snapshot: Value =
        serde_json::from_slice(&due_soon_output).expect("parse due soon decision snapshot");
    let due_soon_task_ids: Vec<_> = due_soon_snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(due_soon_task_ids, vec![due_soon_task_id.as_str()]);

    let overdue_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "overdue_review_decision_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let overdue_snapshot: Value =
        serde_json::from_slice(&overdue_output).expect("parse overdue decision snapshot");
    let overdue_task_ids: Vec<_> = overdue_snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(overdue_task_ids, vec![overdue_task_id.as_str()]);
}

#[test]
fn api_snapshot_review_awaiting_support_tracks_review_tasks_missing_decision_context() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "agent",
            "register",
            "--agent-id",
            "operator",
            "--host-id",
            "operator-host",
            "--host-type",
            "codex",
            "--host-instance",
            "local",
            "--model",
            "gpt-5.4",
            "--project-root",
            "/tmp/project",
            "--worktree-id",
            "wt-1",
        ])
        .assert()
        .success();

    let review_task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Awaiting support task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let review_task: Value = serde_json::from_slice(&review_task_output).expect("parse task");
    let review_task_id = review_task["task_id"]
        .as_str()
        .expect("task id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &review_task_id,
            "--status",
            "review_required",
            "--changed-by",
            "operator",
            "--verification-state",
            "pending",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &review_task_id,
            "--action",
            "post_council_message",
            "--changed-by",
            "operator",
            "--author-agent-id",
            "operator",
            "--message-type",
            "status",
            "--message-body",
            "Waiting on the final support call.",
        ])
        .assert()
        .success();

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_awaiting_support",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let task_ids: Vec<_> = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(task_ids, vec![review_task_id.as_str()]);
    assert!(
        snapshot["task_attention"][0]["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .any(|reason| reason == "review_awaiting_support")
    );
}

#[test]
fn api_snapshot_review_ready_for_decision_tracks_review_tasks_with_support_context() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    let review_task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Ready for decision task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let review_task: Value = serde_json::from_slice(&review_task_output).expect("parse task");
    let review_task_id = review_task["task_id"]
        .as_str()
        .expect("task id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &review_task_id,
            "--status",
            "review_required",
            "--changed-by",
            "operator",
            "--verification-state",
            "pending",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &review_task_id,
            "--action",
            "attach_evidence",
            "--changed-by",
            "operator",
            "--evidence-source-kind",
            "manual_note",
            "--evidence-source-ref",
            "operator-note-1",
            "--evidence-label",
            "Operator note",
        ])
        .assert()
        .success();

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_ready_for_decision",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let task_ids: Vec<_> = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(task_ids, vec![review_task_id.as_str()]);
    assert!(
        snapshot["task_attention"][0]["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .any(|reason| reason == "review_ready_for_decision")
    );

    let detail_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "task",
            "--task-id",
            &review_task_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let detail: Value = serde_json::from_slice(&detail_output).expect("parse task detail");
    let allowed_actions = detail["allowed_actions"]
        .as_array()
        .expect("allowed actions");
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "record_decision")
    );
    assert!(
        !allowed_actions
            .iter()
            .any(|action| action["kind"] == "close_task")
    );
}

#[test]
fn api_snapshot_review_ready_for_closeout_requires_current_cycle_decision_context() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "agent",
            "register",
            "--agent-id",
            "agent-a",
            "--host-id",
            "agent-a",
            "--host-type",
            "codex",
            "--host-instance",
            "local",
            "--model",
            "gpt-5.4",
            "--project-root",
            "/tmp/project",
            "--worktree-id",
            "wt-1",
        ])
        .assert()
        .success();

    let review_task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Ready for closeout task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let review_task: Value = serde_json::from_slice(&review_task_output).expect("parse task");
    let review_task_id = review_task["task_id"]
        .as_str()
        .expect("task id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &review_task_id,
            "--status",
            "review_required",
            "--changed-by",
            "operator",
            "--verification-state",
            "pending",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &review_task_id,
            "--action",
            "attach_evidence",
            "--changed-by",
            "operator",
            "--evidence-source-kind",
            "manual_note",
            "--evidence-source-ref",
            "operator-note-1",
            "--evidence-label",
            "Operator note",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &review_task_id,
            "--action",
            "record_decision",
            "--changed-by",
            "operator",
            "--author-agent-id",
            "agent-a",
            "--message-body",
            "Close the review and ship it.",
        ])
        .assert()
        .success();

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_ready_for_closeout",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let task_ids: Vec<_> = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(task_ids, vec![review_task_id.as_str()]);
    assert!(
        snapshot["task_attention"][0]["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .any(|reason| reason == "review_ready_for_closeout")
    );

    let decision_snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_ready_for_decision",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let decision_snapshot: Value =
        serde_json::from_slice(&decision_snapshot_output).expect("parse decision snapshot");
    assert!(
        decision_snapshot["tasks"]
            .as_array()
            .expect("tasks")
            .is_empty()
    );

    let detail_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "task",
            "--task-id",
            &review_task_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let detail: Value = serde_json::from_slice(&detail_output).expect("parse task detail");
    let allowed_actions = detail["allowed_actions"]
        .as_array()
        .expect("allowed actions");
    assert!(
        allowed_actions
            .iter()
            .any(|action| action["kind"] == "close_task")
    );
    assert!(
        !allowed_actions
            .iter()
            .any(|action| action["kind"] == "record_decision")
    );
}

#[test]
fn api_snapshot_review_ready_for_closeout_excludes_stale_support_from_previous_review_cycle() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    let review_task_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Reopened review task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let review_task: Value = serde_json::from_slice(&review_task_output).expect("parse task");
    let review_task_id = review_task["task_id"]
        .as_str()
        .expect("task id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &review_task_id,
            "--status",
            "review_required",
            "--changed-by",
            "operator",
            "--verification-state",
            "pending",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &review_task_id,
            "--action",
            "attach_evidence",
            "--changed-by",
            "operator",
            "--evidence-source-kind",
            "manual_note",
            "--evidence-source-ref",
            "review-one-note",
            "--evidence-label",
            "First review note",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &review_task_id,
            "--status",
            "in_progress",
            "--changed-by",
            "operator",
            "--verification-state",
            "unknown",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "status",
            "--task-id",
            &review_task_id,
            "--status",
            "review_required",
            "--changed-by",
            "operator",
            "--verification-state",
            "pending",
        ])
        .assert()
        .success();

    let awaiting_support_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_awaiting_support",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let awaiting_support_snapshot: Value =
        serde_json::from_slice(&awaiting_support_output).expect("parse snapshot");
    let awaiting_support_task_ids: Vec<_> = awaiting_support_snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(awaiting_support_task_ids, vec![review_task_id.as_str()]);

    let ready_for_closeout_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--preset",
            "review_ready_for_closeout",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let ready_for_closeout_snapshot: Value =
        serde_json::from_slice(&ready_for_closeout_output).expect("parse snapshot");
    assert!(
        ready_for_closeout_snapshot["tasks"]
            .as_array()
            .expect("tasks")
            .is_empty()
    );
}

#[test]
fn api_snapshot_awaiting_handoff_acceptance_excludes_expired_handoffs() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let now = Utc::now();
    let due_soon_acceptance_due_at = (now + Duration::hours(1)).to_rfc3339();
    let overdue_acceptance_due_at = (now - Duration::hours(1)).to_rfc3339();
    let acceptance_expires_at = (now + Duration::hours(2)).to_rfc3339();

    for agent_id in [
        "agent-a", "agent-b", "agent-c", "agent-d", "agent-e", "agent-f",
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                agent_id,
                "--host-type",
                "codex",
                "--host-instance",
                "local",
                "--model",
                "gpt-5.4",
                "--project-root",
                "/tmp/project",
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    for title in [
        "Pending acceptance",
        "Due soon acceptance",
        "Overdue acceptance",
        "Expired handoff",
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "task",
                "create",
                "--title",
                title,
                "--requested-by",
                "operator",
                "--project-root",
                "/tmp/project",
            ])
            .assert()
            .success();
    }

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let pending_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Pending acceptance")
        .and_then(|task| task["task_id"].as_str())
        .expect("pending task id")
        .to_string();
    let expired_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Expired handoff")
        .and_then(|task| task["task_id"].as_str())
        .expect("expired task id")
        .to_string();
    let due_soon_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Due soon acceptance")
        .and_then(|task| task["task_id"].as_str())
        .expect("due soon task id")
        .to_string();
    let overdue_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Overdue acceptance")
        .and_then(|task| task["task_id"].as_str())
        .expect("overdue task id")
        .to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            &pending_task_id,
            "--from-agent-id",
            "agent-a",
            "--to-agent-id",
            "agent-b",
            "--handoff-type",
            "request_review",
            "--summary",
            "awaiting target agent acceptance",
            "--expires-at",
            "2099-01-01T00:00:00Z",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            &due_soon_task_id,
            "--from-agent-id",
            "agent-a",
            "--to-agent-id",
            "agent-b",
            "--handoff-type",
            "request_review",
            "--summary",
            "due soon acceptance window",
            "--due-at",
            due_soon_acceptance_due_at.as_str(),
            "--expires-at",
            acceptance_expires_at.as_str(),
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            &overdue_task_id,
            "--from-agent-id",
            "agent-a",
            "--to-agent-id",
            "agent-b",
            "--handoff-type",
            "request_review",
            "--summary",
            "overdue acceptance window",
            "--due-at",
            overdue_acceptance_due_at.as_str(),
            "--expires-at",
            acceptance_expires_at.as_str(),
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            &expired_task_id,
            "--from-agent-id",
            "agent-a",
            "--to-agent-id",
            "agent-b",
            "--handoff-type",
            "request_review",
            "--summary",
            "expired before acceptance",
            "--expires-at",
            "2020-01-01T00:00:00Z",
        ])
        .assert()
        .success();

    let awaiting_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "awaiting_handoff_acceptance",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let awaiting_snapshot: Value =
        serde_json::from_slice(&awaiting_output).expect("parse awaiting snapshot");
    let awaiting_task_ids: Vec<_> = awaiting_snapshot["tasks"]
        .as_array()
        .expect("awaiting tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(awaiting_task_ids.len(), 3);
    assert!(awaiting_task_ids.contains(&pending_task_id.as_str()));
    assert!(awaiting_task_ids.contains(&due_soon_task_id.as_str()));
    assert!(awaiting_task_ids.contains(&overdue_task_id.as_str()));
    assert!(
        awaiting_snapshot["task_attention"]
            .as_array()
            .expect("task attention")
            .iter()
            .any(|attention| {
                attention["task_id"] == pending_task_id
                    && attention["reasons"].as_array().is_some_and(|reasons| {
                        reasons
                            .iter()
                            .any(|reason| reason == "awaiting_handoff_acceptance")
                    })
            })
    );

    let due_soon_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "due_soon_handoff_acceptance",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let due_soon_snapshot: Value =
        serde_json::from_slice(&due_soon_output).expect("parse due soon snapshot");
    let due_soon_task_ids: Vec<_> = due_soon_snapshot["tasks"]
        .as_array()
        .expect("due soon tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(due_soon_task_ids, vec![due_soon_task_id.as_str()]);

    let overdue_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "overdue_handoff_acceptance",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let overdue_snapshot: Value =
        serde_json::from_slice(&overdue_output).expect("parse overdue snapshot");
    let overdue_task_ids: Vec<_> = overdue_snapshot["tasks"]
        .as_array()
        .expect("overdue tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(overdue_task_ids, vec![overdue_task_id.as_str()]);

    let handoffs_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "handoffs",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let handoffs_snapshot: Value =
        serde_json::from_slice(&handoffs_output).expect("parse handoff snapshot");
    let handoff_task_ids: Vec<_> = handoffs_snapshot["tasks"]
        .as_array()
        .expect("handoff tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert!(handoff_task_ids.contains(&pending_task_id.as_str()));
    assert!(handoff_task_ids.contains(&expired_task_id.as_str()));
}

#[test]
fn api_snapshot_accepted_handoff_follow_through_splits_due_soon_and_overdue() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let now = Utc::now();
    let due_soon_follow_through_due_at = (now + Duration::hours(1)).to_rfc3339();
    let overdue_follow_through_due_at = (now - Duration::hours(1)).to_rfc3339();
    let stale_overdue_follow_through_due_at = (now - Duration::hours(3)).to_rfc3339();
    let follow_through_expires_at = (now + Duration::hours(2)).to_rfc3339();

    for agent_id in [
        "agent-a", "agent-b", "agent-c", "agent-d", "agent-e", "agent-f",
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "agent",
                "register",
                "--agent-id",
                agent_id,
                "--host-id",
                agent_id,
                "--host-type",
                "codex",
                "--host-instance",
                "local",
                "--model",
                "gpt-5.4",
                "--project-root",
                "/tmp/project",
                "--worktree-id",
                "wt-1",
            ])
            .assert()
            .success();
    }

    for title in [
        "Accepted follow-through",
        "Due soon accepted follow-through",
        "Overdue accepted follow-through",
        "Accepted and already resumed",
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "task",
                "create",
                "--title",
                title,
                "--requested-by",
                "operator",
                "--project-root",
                "/tmp/project",
            ])
            .assert()
            .success();
    }

    let snapshot_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    let accepted_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Accepted follow-through")
        .and_then(|task| task["task_id"].as_str())
        .expect("accepted task id")
        .to_string();
    let due_soon_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Due soon accepted follow-through")
        .and_then(|task| task["task_id"].as_str())
        .expect("due soon task id")
        .to_string();
    let overdue_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Overdue accepted follow-through")
        .and_then(|task| task["task_id"].as_str())
        .expect("overdue task id")
        .to_string();
    let resumed_task_id = snapshot["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == "Accepted and already resumed")
        .and_then(|task| task["task_id"].as_str())
        .expect("resumed task id")
        .to_string();

    let create_transfer_handoff = |task_id: &str, to_agent_id: &str, due_at: Option<&str>| {
        let mut args = vec![
            "--db",
            db_path.to_str().expect("db path"),
            "handoff",
            "create",
            "--task-id",
            task_id,
            "--from-agent-id",
            "agent-a",
            "--to-agent-id",
            to_agent_id,
            "--handoff-type",
            "transfer_ownership",
            "--summary",
            "accepted owner follow-through",
            "--requested-action",
            "pick up execution after transfer",
            "--expires-at",
            follow_through_expires_at.as_str(),
        ];
        if let Some(due_at) = due_at {
            args.extend(["--due-at", due_at]);
        }

        let output = Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args(&args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let handoff: Value = serde_json::from_slice(&output).expect("parse created handoff");
        handoff["handoff_id"]
            .as_str()
            .expect("handoff id")
            .to_string()
    };

    let accepted_handoff_id = create_transfer_handoff(&accepted_task_id, "agent-b", None);
    let due_soon_handoff_id = create_transfer_handoff(
        &due_soon_task_id,
        "agent-c",
        Some(due_soon_follow_through_due_at.as_str()),
    );
    let overdue_handoff_id = create_transfer_handoff(
        &overdue_task_id,
        "agent-d",
        Some(overdue_follow_through_due_at.as_str()),
    );
    let stale_overdue_handoff_id = create_transfer_handoff(
        &overdue_task_id,
        "agent-f",
        Some(stale_overdue_follow_through_due_at.as_str()),
    );
    let resumed_handoff_id = create_transfer_handoff(
        &resumed_task_id,
        "agent-e",
        Some(due_soon_follow_through_due_at.as_str()),
    );

    for (handoff_id, agent_id) in [
        (accepted_handoff_id.as_str(), "agent-b"),
        (due_soon_handoff_id.as_str(), "agent-c"),
        (stale_overdue_handoff_id.as_str(), "agent-f"),
        (overdue_handoff_id.as_str(), "agent-d"),
        (resumed_handoff_id.as_str(), "agent-e"),
    ] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "handoff",
                "resolve",
                "--handoff-id",
                handoff_id,
                "--status",
                "accepted",
                "--resolved-by",
                agent_id,
                "--acting-agent-id",
                agent_id,
            ])
            .assert()
            .success();
    }

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &resumed_task_id,
            "--action",
            "start_task",
            "--changed-by",
            "operator",
            "--acting-agent-id",
            "agent-e",
        ])
        .assert()
        .success();

    let accepted_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "accepted_handoff_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let accepted_snapshot: Value =
        serde_json::from_slice(&accepted_output).expect("parse accepted follow-through snapshot");
    let accepted_task_ids: Vec<_> = accepted_snapshot["tasks"]
        .as_array()
        .expect("accepted tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert!(accepted_task_ids.contains(&accepted_task_id.as_str()));
    assert!(accepted_task_ids.contains(&due_soon_task_id.as_str()));
    assert!(accepted_task_ids.contains(&overdue_task_id.as_str()));
    assert!(!accepted_task_ids.contains(&resumed_task_id.as_str()));
    assert!(
        accepted_snapshot["task_attention"]
            .as_array()
            .expect("task attention")
            .iter()
            .any(|attention| {
                attention["task_id"] == accepted_task_id
                    && attention["reasons"].as_array().is_some_and(|reasons| {
                        reasons
                            .iter()
                            .any(|reason| reason == "accepted_handoff_pending_execution")
                    })
            })
    );

    let due_soon_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "due_soon_accepted_handoff_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let due_soon_snapshot: Value =
        serde_json::from_slice(&due_soon_output).expect("parse due soon accepted snapshot");
    let due_soon_task_ids: Vec<_> = due_soon_snapshot["tasks"]
        .as_array()
        .expect("due soon tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(due_soon_task_ids, vec![due_soon_task_id.as_str()]);

    let overdue_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "overdue_accepted_handoff_follow_through",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let overdue_snapshot: Value =
        serde_json::from_slice(&overdue_output).expect("parse overdue accepted snapshot");
    let overdue_task_ids: Vec<_> = overdue_snapshot["tasks"]
        .as_array()
        .expect("overdue tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(overdue_task_ids, vec![overdue_task_id.as_str()]);
    let overdue_task_sla = overdue_snapshot["task_sla_summaries"]
        .as_array()
        .expect("overdue task sla summaries")
        .iter()
        .find(|summary| summary["task_id"] == overdue_task_id)
        .expect("overdue task sla summary");
    let oldest_overdue_seconds = overdue_task_sla["oldest_overdue_seconds"]
        .as_i64()
        .expect("oldest overdue seconds");
    assert!(oldest_overdue_seconds >= 3_000);
    assert!(oldest_overdue_seconds < 7_200);
}

#[test]
fn api_snapshot_paused_resumable_view_tracks_paused_execution() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "agent",
            "register",
            "--agent-id",
            "agent-a",
            "--host-id",
            "agent-a",
            "--host-type",
            "codex",
            "--host-instance",
            "local",
            "--model",
            "gpt-5.4",
            "--project-root",
            "/tmp/project",
            "--worktree-id",
            "wt-1",
        ])
        .assert()
        .success();

    let create_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Paused task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task: Value = serde_json::from_slice(&create_output).expect("parse task");
    let task_id = task["task_id"].as_str().expect("task id").to_string();

    for action in ["claim_task", "start_task", "pause_task"] {
        Command::cargo_bin("canopy")
            .expect("build canopy binary")
            .args([
                "--db",
                db_path.to_str().expect("db path"),
                "task",
                "action",
                "--task-id",
                &task_id,
                "--action",
                action,
                "--changed-by",
                "operator",
                "--acting-agent-id",
                "agent-a",
            ])
            .assert()
            .success();
    }

    let paused_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "paused_resumable",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let paused_snapshot: Value =
        serde_json::from_slice(&paused_output).expect("parse paused snapshot");
    let paused_task_ids: Vec<_> = paused_snapshot["tasks"]
        .as_array()
        .expect("paused tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(paused_task_ids, vec![task_id.as_str()]);
    assert!(
        paused_snapshot["task_attention"]
            .as_array()
            .expect("task attention")
            .iter()
            .any(|attention| {
                attention["task_id"] == task_id
                    && attention["reasons"].as_array().is_some_and(|reasons| {
                        reasons.iter().any(|reason| reason == "paused_resumable")
                    })
            })
    );
}

#[test]
fn api_snapshot_claimed_not_started_view_tracks_claimed_execution() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "agent",
            "register",
            "--agent-id",
            "agent-a",
            "--host-id",
            "agent-a",
            "--host-type",
            "codex",
            "--host-instance",
            "local",
            "--model",
            "gpt-5.4",
            "--project-root",
            "/tmp/project",
            "--worktree-id",
            "wt-1",
        ])
        .assert()
        .success();

    let create_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Claimed task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task: Value = serde_json::from_slice(&create_output).expect("parse task");
    let task_id = task["task_id"].as_str().expect("task id").to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "action",
            "--task-id",
            &task_id,
            "--action",
            "claim_task",
            "--changed-by",
            "operator",
            "--acting-agent-id",
            "agent-a",
        ])
        .assert()
        .success();

    let claimed_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "claimed_not_started",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let claimed_snapshot: Value =
        serde_json::from_slice(&claimed_output).expect("parse claimed snapshot");
    let claimed_task_ids: Vec<_> = claimed_snapshot["tasks"]
        .as_array()
        .expect("claimed tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(claimed_task_ids, vec![task_id.as_str()]);
    assert!(
        claimed_snapshot["task_attention"]
            .as_array()
            .expect("task attention")
            .iter()
            .any(|attention| {
                attention["task_id"] == task_id
                    && attention["reasons"].as_array().is_some_and(|reasons| {
                        reasons.iter().any(|reason| reason == "claimed_not_started")
                    })
            })
    );
}

#[test]
fn api_snapshot_assigned_awaiting_claim_view_tracks_manual_assignment() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "agent",
            "register",
            "--agent-id",
            "agent-a",
            "--host-id",
            "agent-a",
            "--host-type",
            "codex",
            "--host-instance",
            "local",
            "--model",
            "gpt-5.4",
            "--project-root",
            "/tmp/project",
            "--worktree-id",
            "wt-1",
        ])
        .assert()
        .success();

    let create_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Assigned task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task: Value = serde_json::from_slice(&create_output).expect("parse task");
    let task_id = task["task_id"].as_str().expect("task id").to_string();

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "assign",
            "--task-id",
            &task_id,
            "--assigned-to",
            "agent-a",
            "--assigned-by",
            "operator",
            "--reason",
            "manual assignment before claim",
        ])
        .assert()
        .success();

    let assigned_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "snapshot",
            "--project-root",
            "/tmp/project",
            "--view",
            "assigned_awaiting_claim",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let assigned_snapshot: Value =
        serde_json::from_slice(&assigned_output).expect("parse assigned snapshot");
    let assigned_task_ids: Vec<_> = assigned_snapshot["tasks"]
        .as_array()
        .expect("assigned tasks")
        .iter()
        .map(|task| task["task_id"].as_str().expect("task id"))
        .collect();
    assert_eq!(assigned_task_ids, vec![task_id.as_str()]);
    assert!(
        assigned_snapshot["task_attention"]
            .as_array()
            .expect("task attention")
            .iter()
            .any(|attention| {
                attention["task_id"] == task_id
                    && attention["reasons"].as_array().is_some_and(|reasons| {
                        reasons
                            .iter()
                            .any(|reason| reason == "assigned_awaiting_claim")
                    })
            })
    );
}

#[test]
fn api_snapshot_deadline_presets_and_summaries_follow_runtime_deadlines() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");
    let now = Utc::now();
    let due_soon_at = (now + Duration::hours(12)).to_rfc3339();
    let overdue_at = (now - Duration::hours(2)).to_rfc3339();

    store
        .register_agent(&AgentRegistration {
            agent_id: "codex-1".to_string(),
            host_id: "codex-local".to_string(),
            host_type: "codex".to_string(),
            host_instance: "local".to_string(),
            model: "gpt-5.4".to_string(),
            project_root: "/tmp/project".to_string(),
            worktree_id: "wt-1".to_string(),
            status: AgentStatus::Idle,
            current_task_id: None,
            heartbeat_at: None,
            capabilities: Vec::new(),
            role: None,
        })
        .expect("register execution owner");

    let due_soon_task = store
        .create_task("Execution due soon", None, "operator", "/tmp/project", None)
        .expect("create due soon task");
    store
        .update_task_deadlines(
            &due_soon_task.task_id,
            "operator",
            TaskDeadlineUpdate {
                due_at: Some(due_soon_at.as_str()),
                clear_due_at: false,
                review_due_at: None,
                clear_review_due_at: false,
                event_note: None,
            },
        )
        .expect("set due soon deadline");

    let overdue_execution_task = store
        .create_task("Execution overdue", None, "operator", "/tmp/project", None)
        .expect("create overdue execution task");
    store
        .update_task_deadlines(
            &overdue_execution_task.task_id,
            "operator",
            TaskDeadlineUpdate {
                due_at: Some(overdue_at.as_str()),
                clear_due_at: false,
                review_due_at: None,
                clear_review_due_at: false,
                event_note: None,
            },
        )
        .expect("set overdue execution deadline");
    store
        .assign_task(&overdue_execution_task.task_id, "codex-1", "operator", None)
        .expect("assign overdue execution owner");

    let overdue_execution_unclaimed_task = store
        .create_task(
            "Execution overdue unclaimed",
            None,
            "operator",
            "/tmp/project",
            None,
        )
        .expect("create overdue execution unclaimed task");
    store
        .update_task_deadlines(
            &overdue_execution_unclaimed_task.task_id,
            "operator",
            TaskDeadlineUpdate {
                due_at: Some(overdue_at.as_str()),
                clear_due_at: false,
                review_due_at: None,
                clear_review_due_at: false,
                event_note: None,
            },
        )
        .expect("set overdue execution unclaimed deadline");

    let overdue_review_task = store
        .create_task("Review overdue", None, "operator", "/tmp/project", None)
        .expect("create overdue review task");
    store
        .update_task_status(
            &overdue_review_task.task_id,
            TaskStatus::ReviewRequired,
            "operator",
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Pending),
                ..TaskStatusUpdate::default()
            },
        )
        .expect("move task into review");
    store
        .update_task_deadlines(
            &overdue_review_task.task_id,
            "operator",
            TaskDeadlineUpdate {
                due_at: None,
                clear_due_at: false,
                review_due_at: Some(overdue_at.as_str()),
                clear_review_due_at: false,
                event_note: None,
            },
        )
        .expect("set overdue review deadline");

    let due_soon_review_task = store
        .create_task("Review due soon", None, "operator", "/tmp/project", None)
        .expect("create due soon review task");
    store
        .update_task_status(
            &due_soon_review_task.task_id,
            TaskStatus::ReviewRequired,
            "operator",
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Pending),
                ..TaskStatusUpdate::default()
            },
        )
        .expect("move due soon review task into review");
    store
        .update_task_deadlines(
            &due_soon_review_task.task_id,
            "operator",
            TaskDeadlineUpdate {
                due_at: None,
                clear_due_at: false,
                review_due_at: Some(due_soon_at.as_str()),
                clear_review_due_at: false,
                event_note: None,
            },
        )
        .expect("set due soon review deadline");

    let due_soon_snapshot = api::snapshot(
        &store,
        SnapshotOptions {
            project_root: Some("/tmp/project"),
            preset: Some(SnapshotPreset::DueSoon),
            ..SnapshotOptions::default()
        },
    )
    .expect("load due soon snapshot");
    assert_eq!(due_soon_snapshot.tasks.len(), 2);
    assert_eq!(due_soon_snapshot.deadline_summaries.len(), 2);
    assert_eq!(due_soon_snapshot.sla_summary.due_soon_count, 2);
    assert_eq!(due_soon_snapshot.sla_summary.overdue_count, 0);
    assert_eq!(
        due_soon_snapshot.deadline_summaries[0].active_deadline_state,
        DeadlineState::DueSoon
    );
    assert!(
        due_soon_snapshot
            .task_sla_summaries
            .iter()
            .all(|summary| summary.due_soon_count == 1 && summary.overdue_count == 0)
    );

    let due_soon_execution_snapshot = api::snapshot(
        &store,
        SnapshotOptions {
            project_root: Some("/tmp/project"),
            preset: Some(SnapshotPreset::DueSoonExecution),
            ..SnapshotOptions::default()
        },
    )
    .expect("load due soon execution snapshot");
    assert_eq!(due_soon_execution_snapshot.tasks.len(), 1);
    assert_eq!(
        due_soon_execution_snapshot.tasks[0].task_id,
        due_soon_task.task_id
    );
    assert!(
        due_soon_execution_snapshot.task_attention[0]
            .reasons
            .iter()
            .any(|reason| *reason == TaskAttentionReason::DueSoonExecution)
    );

    let overdue_execution_snapshot = api::snapshot(
        &store,
        SnapshotOptions {
            project_root: Some("/tmp/project"),
            preset: Some(SnapshotPreset::OverdueExecution),
            ..SnapshotOptions::default()
        },
    )
    .expect("load overdue execution snapshot");
    assert_eq!(overdue_execution_snapshot.tasks.len(), 2);
    assert_eq!(overdue_execution_snapshot.sla_summary.due_soon_count, 0);
    assert_eq!(overdue_execution_snapshot.sla_summary.overdue_count, 2);
    assert_eq!(
        overdue_execution_snapshot.sla_summary.breach_severity,
        BreachSeverity::High
    );
    assert!(
        overdue_execution_snapshot
            .tasks
            .iter()
            .any(|task| task.task_id == overdue_execution_task.task_id)
    );
    assert!(
        overdue_execution_snapshot
            .tasks
            .iter()
            .any(|task| task.task_id == overdue_execution_unclaimed_task.task_id)
    );
    assert!(
        overdue_execution_snapshot
            .task_attention
            .iter()
            .flat_map(|attention| attention.reasons.iter())
            .any(|reason| *reason == TaskAttentionReason::OverdueExecution)
    );

    let overdue_execution_owned_snapshot = api::snapshot(
        &store,
        SnapshotOptions {
            project_root: Some("/tmp/project"),
            preset: Some(SnapshotPreset::OverdueExecutionOwned),
            ..SnapshotOptions::default()
        },
    )
    .expect("load overdue execution owned snapshot");
    assert_eq!(overdue_execution_owned_snapshot.tasks.len(), 1);
    assert_eq!(
        overdue_execution_owned_snapshot.tasks[0].task_id,
        overdue_execution_task.task_id
    );
    assert!(
        overdue_execution_owned_snapshot.task_attention[0]
            .reasons
            .iter()
            .any(|reason| *reason == TaskAttentionReason::OverdueExecution)
    );

    let overdue_execution_unclaimed_snapshot = api::snapshot(
        &store,
        SnapshotOptions {
            project_root: Some("/tmp/project"),
            preset: Some(SnapshotPreset::OverdueExecutionUnclaimed),
            ..SnapshotOptions::default()
        },
    )
    .expect("load overdue execution unclaimed snapshot");
    assert_eq!(overdue_execution_unclaimed_snapshot.tasks.len(), 1);
    assert_eq!(
        overdue_execution_unclaimed_snapshot.tasks[0].task_id,
        overdue_execution_unclaimed_task.task_id
    );
    assert!(
        overdue_execution_unclaimed_snapshot.task_attention[0]
            .reasons
            .iter()
            .any(|reason| *reason == TaskAttentionReason::OverdueExecution)
    );

    let overdue_review_snapshot = api::snapshot(
        &store,
        SnapshotOptions {
            project_root: Some("/tmp/project"),
            preset: Some(SnapshotPreset::OverdueReview),
            ..SnapshotOptions::default()
        },
    )
    .expect("load overdue review snapshot");
    assert_eq!(overdue_review_snapshot.tasks.len(), 1);
    assert_eq!(overdue_review_snapshot.sla_summary.overdue_count, 1);
    assert_eq!(
        overdue_review_snapshot.sla_summary.breach_severity,
        BreachSeverity::High
    );
    assert_eq!(
        overdue_review_snapshot.tasks[0].task_id,
        overdue_review_task.task_id
    );
    assert_eq!(
        overdue_review_snapshot.deadline_summaries[0]
            .active_deadline_kind
            .expect("active deadline kind"),
        TaskDeadlineKind::Review
    );
    assert!(
        overdue_review_snapshot.task_attention[0]
            .reasons
            .iter()
            .any(|reason| *reason == TaskAttentionReason::OverdueReview)
    );

    let due_soon_review_snapshot = api::snapshot(
        &store,
        SnapshotOptions {
            project_root: Some("/tmp/project"),
            preset: Some(SnapshotPreset::DueSoonReview),
            ..SnapshotOptions::default()
        },
    )
    .expect("load due soon review snapshot");
    assert_eq!(due_soon_review_snapshot.tasks.len(), 1);
    assert_eq!(
        due_soon_review_snapshot.tasks[0].task_id,
        due_soon_review_task.task_id
    );
    assert!(
        due_soon_review_snapshot.task_attention[0]
            .reasons
            .iter()
            .any(|reason| *reason == TaskAttentionReason::DueSoonReview)
    );

    let detail = api::task_detail(&store, &overdue_review_task.task_id)
        .expect("load overdue review task detail");
    assert_eq!(
        detail.deadline_summary.review_due_at.as_deref(),
        Some(overdue_at.as_str())
    );
    assert!(
        detail
            .allowed_actions
            .iter()
            .any(|action| action.kind == OperatorActionKind::ClearReviewDueAt)
    );
}

#[test]
fn api_exposes_subtask_hierarchy_and_all_children_complete_attention() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    // Parent has verification_required=true but no passing verification yet,
    // so it stays open even after all children complete and the attention
    // reason AllChildrenComplete fires.
    let parent = store
        .create_task_with_options(
            "Parent orchestration task",
            None,
            "operator",
            "/tmp/project",
            &TaskCreationOptions {
                verification_required: true,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create parent task");
    let child_a = store
        .create_subtask(
            &parent.task_id,
            "Implementation child",
            None,
            "operator",
            None,
        )
        .expect("create child a");
    let child_b = store
        .create_subtask(
            &parent.task_id,
            "Verification child",
            None,
            "operator",
            None,
        )
        .expect("create child b");

    for child_id in [&child_a.task_id, &child_b.task_id] {
        store
            .update_task_status(
                child_id,
                TaskStatus::InProgress,
                "operator",
                TaskStatusUpdate::default(),
            )
            .expect("start child task");
        store
            .update_task_status(
                child_id,
                TaskStatus::Completed,
                "operator",
                TaskStatusUpdate {
                    verification_state: Some(VerificationState::Passed),
                    closure_summary: Some("done"),
                    ..TaskStatusUpdate::default()
                },
            )
            .expect("complete child task");
    }

    let snapshot = api::snapshot(
        &store,
        SnapshotOptions {
            project_root: Some("/tmp/project"),
            ..SnapshotOptions::default()
        },
    )
    .expect("load snapshot");
    let parent_attention = snapshot
        .task_attention
        .iter()
        .find(|attention| attention.task_id == parent.task_id)
        .expect("parent attention");
    assert!(
        parent_attention
            .reasons
            .contains(&TaskAttentionReason::AllChildrenComplete)
    );
    let parent_relationship_summary = snapshot
        .relationship_summaries
        .iter()
        .find(|summary| summary.task_id == parent.task_id)
        .expect("parent relationship summary");
    assert_eq!(parent_relationship_summary.child_count, 2);
    assert_eq!(parent_relationship_summary.open_child_count, 0);
    assert!(parent_relationship_summary.children_complete);

    let parent_detail = api::task_detail(&store, &parent.task_id).expect("load parent task detail");
    assert_eq!(parent_detail.children.len(), 2);
    assert!(parent_detail.children_complete);
    assert!(parent_detail.parent_id.is_none());
    assert!(
        parent_detail
            .children
            .iter()
            .any(|child| child.task_id == child_a.task_id && child.status == TaskStatus::Completed)
    );
    assert!(
        parent_detail
            .children
            .iter()
            .any(|child| child.task_id == child_b.task_id && child.status == TaskStatus::Completed)
    );

    let child_detail = api::task_detail(&store, &child_a.task_id).expect("load child detail");
    assert_eq!(
        child_detail.parent_id.as_deref(),
        Some(parent.task_id.as_str())
    );
    assert!(child_detail.children.is_empty());
    assert!(!child_detail.children_complete);
}
