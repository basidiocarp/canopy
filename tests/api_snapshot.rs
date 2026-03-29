use assert_cmd::Command;
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
        4
    );
    assert_eq!(snapshot["operator_actions"][0]["kind"], "acknowledge_task");
    assert_eq!(snapshot["operator_actions"][0]["target_kind"], "task");
    assert_eq!(snapshot["operator_actions"][1]["kind"], "verify_task");
    assert_eq!(snapshot["operator_actions"][1]["target_kind"], "task");
    assert_eq!(
        snapshot["operator_actions"][2]["kind"],
        "resolve_dependency"
    );
    assert_eq!(snapshot["operator_actions"][2]["target_kind"], "task");
    assert_eq!(snapshot["operator_actions"][3]["kind"], "promote_follow_up");
    assert_eq!(snapshot["operator_actions"][3]["target_kind"], "task");
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
    assert_eq!(event_types.len(), 6);
    assert!(event_types.contains(&"created"));
    assert!(event_types.contains(&"assigned"));
    assert!(event_types.contains(&"status_changed"));
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
        4
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
            .any(|action| action["kind"] == "post_council_message")
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
fn api_snapshot_awaiting_handoff_acceptance_excludes_expired_handoffs() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

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

    for title in ["Pending acceptance", "Expired handoff"] {
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
    assert_eq!(awaiting_task_ids, vec![pending_task_id.as_str()]);
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
