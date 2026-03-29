use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn cli_registers_agents_and_lists_them() {
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
            "codex-1",
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
        .success()
        .stdout(predicate::str::contains("\"agent_id\": \"codex-1\""));

    let output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args(["--db", db_path.to_str().expect("db path"), "agent", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let agents: Value = serde_json::from_slice(&output).expect("parse agent list");
    let first = &agents[0];
    assert_eq!(first["agent_id"], "codex-1");
    assert_eq!(first["host_id"], "codex-local");
    assert_eq!(first["host_type"], "codex");
    assert_eq!(first["status"], "idle");
    assert!(first["heartbeat_at"].is_string());

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "agent",
            "history",
            "--agent-id",
            "codex-1",
            "--limit",
            "5",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"source\": \"register\""));
}

#[test]
fn cli_creates_and_resolves_handoffs() {
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
            "Review operator contract",
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
            "ask for contract review",
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
    assert_eq!(handoff["status"], "open");

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
            "claude-1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"accepted\""));

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
            "completed",
            "--changed-by",
            "claude-1",
            "--verification-state",
            "passed",
            "--closure-summary",
            "review accepted",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"verification_state\": \"passed\"",
        ))
        .stdout(predicate::str::contains("\"closed_by\": \"claude-1\""));

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "list-view",
            "--project-root",
            "/tmp/project",
            "--view",
            "review",
            "--sort",
            "updated_at",
        ])
        .assert()
        .success();
}

#[test]
fn cli_rejects_invalid_council_message_type() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "council",
            "post",
            "--task-id",
            "missing-task",
            "--author-agent-id",
            "missing-agent",
            "--message-type",
            "not_a_real_type",
            "--body",
            "bad",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("possible values"));
}

#[test]
fn cli_task_creation_actions_flow_through_task_action_command() {
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
            "Coordinate next task",
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
            "action",
            "--task-id",
            &task_id,
            "--action",
            "create_handoff",
            "--changed-by",
            "operator",
            "--from-agent-id",
            "codex-1",
            "--to-agent-id",
            "claude-1",
            "--handoff-type",
            "request_review",
            "--handoff-summary",
            "review the operator plan",
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
            "post_council_message",
            "--changed-by",
            "operator",
            "--author-agent-id",
            "codex-1",
            "--message-type",
            "status",
            "--message-body",
            "Coordination started.",
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
            &task_id,
            "--action",
            "create_follow_up_task",
            "--changed-by",
            "operator",
            "--follow-up-title",
            "Finish review fallout",
            "--follow-up-description",
            "Track the remaining coordination work.",
        ])
        .assert()
        .success();

    Command::cargo_bin("canopy")
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
        .stdout(predicate::str::contains("\"handoff_created\""))
        .stdout(predicate::str::contains("\"council_message_posted\""))
        .stdout(predicate::str::contains("\"evidence_attached\""))
        .stdout(predicate::str::contains("\"follow_up_task_created\""));
}

#[test]
fn cli_requires_blocked_reason_for_blocked_status() {
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
            "blocked",
            "--changed-by",
            "operator",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "blocked tasks require a blocked reason",
        ));
}

#[test]
fn cli_updates_triage_metadata_and_supports_due_handoffs() {
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
            "Triage task",
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
            "triage",
            "--task-id",
            &task_id,
            "--changed-by",
            "operator",
            "--priority",
            "high",
            "--severity",
            "critical",
            "--acknowledged",
            "false",
            "--owner-note",
            "handoff to strongest verifier",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"priority\": \"high\""))
        .stdout(predicate::str::contains("\"severity\": \"critical\""))
        .stdout(predicate::str::contains(
            "\"owner_note\": \"handoff to strongest verifier\"",
        ));

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
            "review this critical task",
            "--due-at",
            "2000-01-01T00:00:00Z",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"due_at\": \"2000-01-01T00:00:00Z\"",
        ));
}

#[test]
fn cli_applies_operator_actions_and_records_runtime_history() {
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
            "Operator task",
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
            "task",
            "action",
            "--task-id",
            &task_id,
            "--action",
            "acknowledge_task",
            "--changed-by",
            "operator",
            "--note",
            "triage started",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"acknowledged_by\": \"operator\"",
        ));

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
            "reassign_task",
            "--changed-by",
            "operator",
            "--assigned-to",
            "claude-1",
            "--note",
            "handoff to reviewer",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"owner_agent_id\": \"claude-1\""));

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
            "verify_task",
            "--changed-by",
            "operator",
            "--verification-state",
            "failed",
            "--note",
            "premature review attempt",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "verify_task requires a task that is awaiting or repeating review",
        ));

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
            "verify_task",
            "--changed-by",
            "operator",
            "--verification-state",
            "passed",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "verify_task passed reviews require a closure summary",
        ));

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
            "verify_task",
            "--changed-by",
            "operator",
            "--verification-state",
            "passed",
            "--closure-summary",
            "operator review accepted the task",
            "--note",
            "review completed in canopy",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"completed\""))
        .stdout(predicate::str::contains(
            "\"verification_state\": \"passed\"",
        ));

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
            "claude-1",
            "--to-agent-id",
            "codex-1",
            "--handoff-type",
            "request_review",
            "--summary",
            "check the change",
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
            "handoff",
            "action",
            "--handoff-id",
            &handoff_id,
            "--action",
            "follow_up_handoff",
            "--changed-by",
            "operator",
            "--note",
            "need review before release",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"open\""));

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
    let events = detail["events"].as_array().expect("events");
    assert!(
        events.iter().any(|event| {
            event["event_type"] == "triage_updated"
                && event["note"]
                    .as_str()
                    .is_some_and(|note| note.contains("acknowledged:false->true"))
        }),
        "expected acknowledge history event"
    );
    assert!(
        events.iter().any(|event| {
            event["event_type"] == "ownership_transferred"
                && event["note"]
                    .as_str()
                    .is_some_and(|note| note.contains("owner:codex-1->claude-1"))
        }),
        "expected reassignment history event"
    );
    assert!(
        events.iter().any(|event| {
            event["event_type"] == "status_changed"
                && event["verification_state"] == "passed"
                && event["note"]
                    .as_str()
                    .is_some_and(|note| note.contains("operator review accepted the task"))
        }),
        "expected verify history event"
    );
    assert!(
        events.iter().any(|event| {
            event["event_type"] == "handoff_updated"
                && event["note"]
                    .as_str()
                    .is_some_and(|note| note.contains("handoff_action=follow_up"))
        }),
        "expected handoff follow-up history event"
    );
}
