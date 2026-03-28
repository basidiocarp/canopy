use assert_cmd::Command;
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
    assert_eq!(snapshot["agents"].as_array().expect("agents").len(), 2);
    assert_eq!(
        snapshot["heartbeats"].as_array().expect("heartbeats").len(),
        2
    );
    assert_eq!(snapshot["tasks"].as_array().expect("tasks").len(), 1);
    assert_eq!(snapshot["handoffs"].as_array().expect("handoffs").len(), 1);
    assert_eq!(snapshot["evidence"].as_array().expect("evidence").len(), 1);

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
    assert_eq!(detail["events"].as_array().expect("events").len(), 2);
    assert_eq!(detail["events"][0]["event_type"], "created");
    assert_eq!(detail["events"][1]["event_type"], "status_changed");
    assert!(
        detail["heartbeats"]
            .as_array()
            .expect("heartbeats")
            .is_empty()
    );
    assert_eq!(detail["handoffs"].as_array().expect("handoffs").len(), 1);
    assert_eq!(detail["evidence"].as_array().expect("evidence").len(), 1);
    assert_eq!(detail["evidence"][0]["related_session_id"], "ses_123");
    assert!(detail["evidence"][0]["related_memory_query"].is_null());
}

#[test]
fn api_snapshot_status_sort_uses_operator_priority_order() {
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
        .success();

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
