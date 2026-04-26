//! Septa contract producer tests for Canopy read models.
//!
//! Validates that `ApiSnapshot` and task detail serializations conform to
//! septa contracts: required fields are present and rich types are serialized
//! correctly (e.g., allowed_actions as objects, not strings).

use canopy::models::{
    OperatorAction, OperatorActionKind, OperatorActionTargetKind, AttentionLevel,
    SnapshotAttentionSummary,
};

#[test]
fn test_snapshot_attention_summary_has_needs_verification_count() {
    let summary = SnapshotAttentionSummary {
        tasks_needing_attention: 1,
        critical_tasks: 0,
        handoffs_needing_attention: 0,
        stale_handoffs: 0,
        agents_needing_attention: 0,
        stale_agents: 0,
        actionable_tasks: 1,
        actionable_handoffs: 0,
        needs_verification_count: 5,
    };

    let json = serde_json::to_value(&summary).expect("summary must serialize");
    assert!(json.is_object(), "summary must be object");

    let obj = json.as_object().expect("summary must be object");

    // Verify needs_verification_count is serialized
    assert!(
        obj.get("needs_verification_count").is_some(),
        "needs_verification_count must be present"
    );
    assert_eq!(
        obj.get("needs_verification_count")
            .and_then(|v| v.as_u64()),
        Some(5),
        "needs_verification_count must serialize as integer"
    );
}

#[test]
fn test_operator_action_serialization_is_object_not_string() {
    let action = OperatorAction {
        action_id: "act-001".to_string(),
        kind: OperatorActionKind::ReassignTask,
        target_kind: OperatorActionTargetKind::Task,
        level: AttentionLevel::NeedsAttention,
        task_id: Some("task-123".to_string()),
        handoff_id: None,
        agent_id: None,
        title: "Reassign to reviewer".to_string(),
        summary: "Transfer this task to review queue".to_string(),
        due_at: None,
        expires_at: Some("2026-04-26T00:00:00Z".to_string()),
    };

    let json = serde_json::to_value(&action).expect("action must serialize");

    // Verify it serializes as an object, not a string
    assert!(json.is_object(), "action must serialize to object");

    let obj = json.as_object().expect("action must be object");

    // Verify required fields are present
    assert!(obj.get("action_id").is_some(), "action_id required");
    assert!(obj.get("kind").is_some(), "kind required");
    assert!(obj.get("target_kind").is_some(), "target_kind required");
    assert!(obj.get("level").is_some(), "level required");
    assert!(obj.get("title").is_some(), "title required");
    assert!(obj.get("summary").is_some(), "summary required");

    // Verify kinds serialize as strings (snake_case)
    assert!(
        obj.get("kind")
            .and_then(|k| k.as_str())
            .is_some(),
        "kind must serialize as string"
    );
    assert!(
        obj.get("target_kind")
            .and_then(|tk| tk.as_str())
            .is_some(),
        "target_kind must serialize as string"
    );
    let level_str = obj
        .get("level")
        .and_then(|l| l.as_str())
        .expect("level must be present and a string");
    assert!(
        ["normal", "needs_attention", "critical"].contains(&level_str),
        "level must be a valid AttentionLevel value, got: {}",
        level_str
    );
}

#[test]
fn test_allowed_actions_array_contains_objects() {
    let actions = vec![OperatorAction {
        action_id: "act-001".to_string(),
        kind: OperatorActionKind::ReassignTask,
        target_kind: OperatorActionTargetKind::Task,
        level: AttentionLevel::NeedsAttention,
        task_id: Some("task-123".to_string()),
        handoff_id: None,
        agent_id: None,
        title: "Reassign".to_string(),
        summary: "Reassign this task".to_string(),
        due_at: None,
        expires_at: None,
    }];

    let json_array = serde_json::to_value(&actions).expect("actions must serialize");

    // Verify it serializes as array
    let arr = json_array.as_array().expect("must be array");
    assert!(!arr.is_empty(), "array must contain at least one item");

    // Verify first item is object with action_id
    let first = &arr[0];
    assert!(first.is_object(), "array items must be objects");
    assert!(
        first.get("action_id").is_some(),
        "action_id must be present in allowed_actions objects"
    );
}
