use assert_cmd::Command;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn api_snapshot_includes_agents_tasks_handoffs_and_evidence() {
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
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let snapshot: Value = serde_json::from_slice(&snapshot_output).expect("parse snapshot");
    assert_eq!(snapshot["agents"].as_array().expect("agents").len(), 2);
    assert_eq!(snapshot["tasks"].as_array().expect("tasks").len(), 1);
    assert_eq!(snapshot["handoffs"].as_array().expect("handoffs").len(), 1);
    assert_eq!(snapshot["evidence"].as_array().expect("evidence").len(), 1);
}
