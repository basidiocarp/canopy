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
        vec!["review_required", "unacknowledged"]
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
        2
    );
    assert_eq!(snapshot["operator_actions"][0]["kind"], "acknowledge_task");
    assert_eq!(snapshot["operator_actions"][0]["target_kind"], "task");
    assert_eq!(snapshot["operator_actions"][1]["kind"], "verify_task");
    assert_eq!(snapshot["operator_actions"][1]["target_kind"], "task");
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
    assert_eq!(detail["attention"]["level"], "needs_attention");
    assert_eq!(detail["attention"]["reasons"][0], "review_required");
    assert_eq!(detail["events"].as_array().expect("events").len(), 3);
    assert_eq!(detail["events"][0]["event_type"], "created");
    assert_eq!(detail["events"][1]["event_type"], "assigned");
    assert_eq!(detail["events"][2]["event_type"], "status_changed");
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
        2
    );
    let allowed_actions = detail["allowed_actions"]
        .as_array()
        .expect("allowed actions");
    assert!(allowed_actions.iter().any(|action| action["kind"] == "acknowledge_task"));
    assert!(allowed_actions.iter().any(|action| action["kind"] == "verify_task"));
    assert!(allowed_actions.iter().any(|action| action["kind"] == "reassign_task"));
    assert!(allowed_actions.iter().any(|action| action["kind"] == "set_task_priority"));
    assert!(allowed_actions.iter().any(|action| action["kind"] == "set_task_severity"));
    assert!(allowed_actions.iter().any(|action| action["kind"] == "update_task_note"));
    assert!(allowed_actions.iter().any(|action| action["kind"] == "block_task"));
    assert!(allowed_actions.iter().any(|action| action["kind"] == "follow_up_handoff"));
    assert!(allowed_actions.iter().any(|action| action["kind"] == "expire_handoff"));
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

#[test]
fn api_snapshot_attention_view_returns_only_tasks_needing_attention() {
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
            "codex-1",
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
    assert_eq!(snapshot["agent_attention"][0]["freshness"], "stale");
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
