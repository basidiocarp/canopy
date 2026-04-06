use assert_cmd::Command;
use predicates::prelude::*;
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::Path;
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
    assert_eq!(first["freshness"], "fresh");

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
fn cli_agent_register_supports_role_flag() {
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
            "--role",
            "implementer",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"role\": \"implementer\""));
}

#[test]
fn cli_agent_register_supports_capabilities_flag() {
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
            "--capabilities",
            "rust,hyphae,sqlite",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"capabilities\": [\n    \"rust\",\n    \"hyphae\",\n    \"sqlite\"",
        ));
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
            "--acting-agent-id",
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
            "in_progress",
            "--changed-by",
            "claude-1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"in_progress\""));

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
            "attach_evidence",
            "--changed-by",
            "operator",
            "--evidence-source-kind",
            "manual_note",
            "--evidence-source-ref",
            "review-note-1",
            "--evidence-label",
            "Review note",
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
            "set_task_due_at",
            "--changed-by",
            "operator",
            "--due-at",
            "2026-04-01T00:00:00Z",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "set_task_due_at requires a non-terminal task outside review",
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
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "verify_task no longer accepts passed; use close_task",
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
            "attach_evidence",
            "--changed-by",
            "operator",
            "--evidence-source-kind",
            "manual_note",
            "--evidence-source-ref",
            "review-note-1",
            "--evidence-label",
            "Review note",
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
            "record_decision",
            "--changed-by",
            "operator",
            "--author-agent-id",
            "claude-1",
            "--message-body",
            "Decision recorded for closeout.",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"review_required\""));

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
            "close_task",
            "--changed-by",
            "operator",
            "--closure-summary",
            "operator review accepted the task",
            "--note",
            "review closeout completed in canopy",
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

#[test]
fn cli_sets_and_clears_task_and_review_deadlines() {
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
            "Track deadline controls",
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
            "set_task_due_at",
            "--changed-by",
            "operator",
            "--due-at",
            "2026-03-30T18:00:00Z",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"due_at\": \"2026-03-30T18:00:00Z\"",
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
            "set_review_due_at",
            "--changed-by",
            "operator",
            "--review-due-at",
            "2026-03-31T12:00:00Z",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"review_due_at\": \"2026-03-31T12:00:00Z\"",
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
            "clear_task_due_at",
            "--changed-by",
            "operator",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"due_at\": null"));

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
            "clear_review_due_at",
            "--changed-by",
            "operator",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"review_due_at\": null"));
}

#[test]
fn cli_task_create_supports_parent_flag() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    let parent_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Parent task",
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
    let parent: Value = serde_json::from_slice(&parent_output).expect("parse parent task");
    let parent_id = parent["task_id"].as_str().expect("parent id").to_string();

    let child_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Child task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/ignored",
            "--parent",
            &parent_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let child: Value = serde_json::from_slice(&child_output).expect("parse child task");
    let child_id = child["task_id"].as_str().expect("child id").to_string();
    assert_eq!(child["project_root"], "/tmp/project");

    let detail_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "task",
            "--task-id",
            &parent_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let detail: Value = serde_json::from_slice(&detail_output).expect("parse task detail");
    assert_eq!(detail["children_complete"], false);
    assert!(
        detail["children"]
            .as_array()
            .expect("children array")
            .iter()
            .any(|child| child["task_id"] == child_id && child["status"] == "open")
    );

    let child_detail_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "api",
            "task",
            "--task-id",
            &child_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let child_detail: Value =
        serde_json::from_slice(&child_detail_output).expect("parse child task detail");
    assert_eq!(child_detail["parent_id"], parent_id);
}

#[test]
fn cli_task_create_supports_required_role_flag() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Validation task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
            "--required-role",
            "validator",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"required_role\": \"validator\""));
}

#[test]
fn cli_task_create_supports_required_capabilities_flag() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Capability task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
            "--required-capabilities",
            "rust,hyphae",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"required_capabilities\": [\n    \"rust\",\n    \"hyphae\"",
        ));
}

#[test]
fn cli_task_create_supports_auto_review_flag() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "create",
            "--title",
            "Implementation task",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
            "--auto-review",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"auto_review\": true"));
}

#[test]
fn cli_import_handoff_creates_task_tree_with_verification_metadata() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let handoff_path = temp.path().join("verification-enforcement.md");
    let verify_script_path = temp.path().join("verify-verification-enforcement.sh");

    fs::write(
        &handoff_path,
        "\
# Handoff: Example Handoff

### Step 1: First step
Implement the first step.

### Step 2: Second step
Implement the second step.
",
    )
    .expect("write handoff");
    fs::write(
        &verify_script_path,
        "#!/bin/bash\necho 'Results: 0 passed, 0 failed'\n",
    )
    .expect("write verify script");

    let output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "import-handoff",
            handoff_path.to_str().expect("handoff path"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let imported: Value = serde_json::from_slice(&output).expect("parse import output");
    assert_eq!(imported["parent_task"]["title"], "Example Handoff");
    assert_eq!(imported["parent_task"]["verification_required"], true);
    assert_eq!(imported["steps"].as_array().expect("steps array").len(), 2);
    assert_eq!(imported["steps"][0]["title"], "Step 1: First step");

    let parent_task_id = imported["parent_task"]["task_id"]
        .as_str()
        .expect("parent task id");
    let evidence_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "evidence",
            "list",
            "--task-id",
            parent_task_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let evidence: Value = serde_json::from_slice(&evidence_output).expect("parse evidence list");
    assert_eq!(evidence[0]["label"], "Verification command");
}

#[test]
fn cli_task_verify_records_script_evidence_and_completes_leaf_task() {
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
            "Verified step",
            "--requested-by",
            "operator",
            "--project-root",
            "/tmp/project",
            "--verification-required",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task: Value = serde_json::from_slice(&task_output).expect("parse task");
    let task_id = task["task_id"].as_str().expect("task id").to_string();

    let verify_script_path = temp.path().join("verify-step.sh");
    fs::write(
        &verify_script_path,
        "#!/bin/bash\necho '--- Step 1: Verified step ---'\necho '  PASS: check'\necho 'Results: 1 passed, 0 failed'\n",
    )
    .expect("write verify script");

    let verify_output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "verify",
            "--task-id",
            &task_id,
            "--script",
            verify_script_path.to_str().expect("verify script path"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let result: Value = serde_json::from_slice(&verify_output).expect("parse verify output");
    assert_eq!(result["passed"], true);
    assert_eq!(result["task"]["status"], "completed");
    assert_eq!(result["task"]["verification_state"], "passed");

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "evidence",
            "verify",
            "--task-id",
            &task_id,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"source_kind\": \"script_verification\"",
        ))
        .stdout(predicate::str::contains("\"status\": \"verified\""));
}

#[test]
fn cli_task_claim_rejects_stale_agent_without_force() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    register_agent(&db_path, "codex-1");
    let task_id = create_task(&db_path, "Stale claim task");
    age_agent_heartbeat(&db_path, "codex-1", 10);

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "claim",
            "--agent-id",
            "codex-1",
            &task_id,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("last heartbeat was"))
        .stderr(predicate::str::contains("threshold: 300s"));
}

#[test]
fn cli_task_claim_force_claim_bypasses_freshness_check() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    register_agent(&db_path, "codex-1");
    let task_id = create_task(&db_path, "Forced claim task");
    age_agent_heartbeat(&db_path, "codex-1", 10);

    Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args([
            "--db",
            db_path.to_str().expect("db path"),
            "task",
            "claim",
            "--agent-id",
            "codex-1",
            "--force-claim",
            &task_id,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"owner_agent_id\": \"codex-1\""));
}

#[test]
fn cli_agent_list_reports_aging_before_stale() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    register_agent(&db_path, "codex-1");
    age_agent_heartbeat(&db_path, "codex-1", 20);

    let output = Command::cargo_bin("canopy")
        .expect("build canopy binary")
        .args(["--db", db_path.to_str().expect("db path"), "agent", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let agents: Value = serde_json::from_slice(&output).expect("parse agent list");
    assert_eq!(agents[0]["freshness"], "aging");
}

fn register_agent(db_path: &Path, agent_id: &str) {
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

fn create_task(db_path: &Path, title: &str) -> String {
    let output = Command::cargo_bin("canopy")
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
    let task: Value = serde_json::from_slice(&output).expect("parse task");
    task["task_id"].as_str().expect("task id").to_string()
}

fn age_agent_heartbeat(db_path: &Path, agent_id: &str, minutes_ago: i64) {
    let conn = Connection::open(db_path).expect("open db");
    conn.execute(
        "UPDATE agents SET heartbeat_at = datetime('now', ?1) WHERE agent_id = ?2",
        [format!("-{minutes_ago} minutes"), agent_id.to_string()],
    )
    .expect("age agent heartbeat");
}
