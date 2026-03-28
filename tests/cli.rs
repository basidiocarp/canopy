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
