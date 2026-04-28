use canopy::models::{Notification, NotificationEventType};
use serde_json::json;

#[test]
fn notification_serialization_contract() {
    let notification = Notification {
        notification_id: "01HZ1234567890ABCDEF012345".to_string(),
        event_type: NotificationEventType::TaskCancelled,
        task_id: Some("01HZ0987654321FEDCBA987654".to_string()),
        agent_id: Some("claude-code-session-abc".to_string()),
        payload: json!({
            "reason": "Superseded by higher-priority handoff"
        }),
        seen: false,
        created_at: "2026-04-16T12:00:00Z".to_string(),
    };

    let value = serde_json::to_value(&notification).expect("Failed to serialize notification");

    // Verify notification_id field is present
    assert!(
        value.get("notification_id").is_some(),
        "notification_id field must be present"
    );
    assert_eq!(
        value.get("notification_id").and_then(|v| v.as_str()),
        Some("01HZ1234567890ABCDEF012345")
    );

    // Verify id field is NOT present (schema uses notification_id, not id)
    assert!(
        value.get("id").is_none(),
        "field 'id' must not be present; use 'notification_id' instead"
    );

    // Verify seen field is present and is a boolean
    assert!(value.get("seen").is_some(), "seen field must be present");
    assert_eq!(
        value.get("seen").and_then(serde_json::Value::as_bool),
        Some(false),
        "seen field must be a boolean with value false"
    );

    // Verify read_at field is NOT present (schema uses seen, not read_at)
    assert!(
        value.get("read_at").is_none(),
        "field 'read_at' must not be present; use 'seen' instead"
    );

    // Verify event_type is present
    assert_eq!(
        value.get("event_type").and_then(|v| v.as_str()),
        Some("task_cancelled"),
        "event_type field must be present and correct"
    );

    // Verify payload is present
    assert_eq!(
        value
            .get("payload")
            .and_then(|v| v.get("reason"))
            .and_then(|v| v.as_str()),
        Some("Superseded by higher-priority handoff"),
        "payload field must be present with correct reason"
    );

    // Verify created_at is present
    assert_eq!(
        value.get("created_at").and_then(|v| v.as_str()),
        Some("2026-04-16T12:00:00Z"),
        "created_at field must be present and correct"
    );

    // Verify optional fields (task_id, agent_id) are present
    assert!(
        value.get("task_id").is_some(),
        "task_id field must be present"
    );
    assert!(
        value.get("agent_id").is_some(),
        "agent_id field must be present"
    );
}

#[test]
fn notification_deserialization_contract() {
    let json = json!({
        "notification_id": "01HZ1234567890ABCDEF012345",
        "event_type": "task_cancelled",
        "task_id": "01HZ0987654321FEDCBA987654",
        "agent_id": "claude-code-session-abc",
        "payload": {
            "reason": "Superseded by higher-priority handoff"
        },
        "created_at": "2026-04-16T12:00:00Z",
        "seen": false
    });

    let notification: Notification =
        serde_json::from_value(json).expect("Failed to deserialize notification");

    assert_eq!(notification.notification_id, "01HZ1234567890ABCDEF012345");
    assert_eq!(
        notification.event_type,
        NotificationEventType::TaskCancelled
    );
    assert_eq!(
        notification.task_id,
        Some("01HZ0987654321FEDCBA987654".to_string())
    );
    assert_eq!(
        notification.agent_id,
        Some("claude-code-session-abc".to_string())
    );
    assert!(!notification.seen);
    assert_eq!(notification.created_at, "2026-04-16T12:00:00Z");
}
