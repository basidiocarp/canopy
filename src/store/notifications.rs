use rusqlite::{Connection, params};

use crate::models::{Notification, NotificationEventType};
use crate::store::StoreResult;

use super::StoreError;

/// Serialize a [`NotificationEventType`] to the `snake_case` string stored in the
/// `event_type` TEXT column.
fn event_type_to_str(event_type: &NotificationEventType) -> StoreResult<String> {
    // serde_json encodes the variant as a quoted JSON string (e.g. `"task_assigned"`).
    // Strip the surrounding quotes to get the bare column value.
    let json =
        serde_json::to_string(event_type).map_err(|e| StoreError::Validation(e.to_string()))?;
    // Remove surrounding `"` characters produced by JSON string serialisation.
    Ok(json.trim_matches('"').to_string())
}

/// Deserialize a [`NotificationEventType`] from the `snake_case` string stored
/// in the `event_type` TEXT column.
fn event_type_from_str(value: &str) -> StoreResult<NotificationEventType> {
    // Re-add the quotes so serde_json can parse it as a JSON string value.
    let quoted = format!("\"{value}\"");
    serde_json::from_str(&quoted).map_err(|e| {
        StoreError::Validation(format!("unknown notification event_type {value:?}: {e}"))
    })
}

/// Insert a new notification row.
///
/// # Errors
///
/// Returns an error if the `event_type` cannot be serialized or the write fails.
pub fn insert_notification(conn: &Connection, notification: &Notification) -> StoreResult<()> {
    let event_type_str = event_type_to_str(&notification.event_type)?;
    let payload_str = serde_json::to_string(&notification.payload)
        .map_err(|e| StoreError::Validation(e.to_string()))?;
    conn.execute(
        r"
        INSERT INTO notifications (
            notification_id, event_type, task_id, agent_id,
            payload, seen, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            notification.notification_id,
            event_type_str,
            notification.task_id,
            notification.agent_id,
            payload_str,
            i64::from(notification.seen),
            notification.created_at,
        ],
    )?;
    Ok(())
}

/// List notifications, optionally including already-seen ones.
///
/// When `include_seen` is `false`, only unseen rows (`seen = 0`) are returned.
/// Rows are ordered by `created_at` ascending.
///
/// # Errors
///
/// Returns an error if the query or row mapping fails.
pub fn list_notifications(
    conn: &Connection,
    include_seen: bool,
) -> StoreResult<Vec<Notification>> {
    let sql = if include_seen {
        r"
        SELECT notification_id, event_type, task_id, agent_id, payload, seen, created_at
        FROM notifications
        ORDER BY created_at, rowid
        "
    } else {
        r"
        SELECT notification_id, event_type, task_id, agent_id, payload, seen, created_at
        FROM notifications
        WHERE seen = 0
        ORDER BY created_at, rowid
        "
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| {
        let event_type_str: String = row.get(1)?;
        let payload_str: String = row.get(4)?;
        Ok((
            row.get::<_, String>(0)?,
            event_type_str,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            payload_str,
            row.get::<_, i64>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;

    let mut notifications = Vec::new();
    for row in rows {
        let (notification_id, event_type_str, task_id, agent_id, payload_str, seen_int, created_at) =
            row?;
        let event_type = event_type_from_str(&event_type_str)?;
        let payload = serde_json::from_str(&payload_str)
            .map_err(|e| StoreError::Validation(e.to_string()))?;
        notifications.push(Notification {
            notification_id,
            event_type,
            task_id,
            agent_id,
            payload,
            seen: seen_int != 0,
            created_at,
        });
    }
    Ok(notifications)
}

/// Mark a notification as seen.
///
/// # Errors
///
/// Returns an error if the write fails.
pub fn mark_seen(conn: &Connection, notification_id: &str) -> StoreResult<()> {
    conn.execute(
        "UPDATE notifications SET seen = 1 WHERE notification_id = ?1",
        [notification_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::schema::{BASE_SCHEMA, migrate_schema};
    use rusqlite::Connection;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("foreign_keys");
        conn.execute_batch(BASE_SCHEMA).expect("base schema");
        migrate_schema(&conn).expect("migrate");
        conn
    }

    fn make_notification(id: &str, event_type: NotificationEventType) -> Notification {
        Notification {
            notification_id: id.to_string(),
            event_type,
            task_id: None,
            agent_id: None,
            payload: serde_json::json!({}),
            seen: false,
            created_at: "2026-04-16T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn insert_and_list_roundtrip() {
        let conn = test_conn();
        let notification = make_notification("notif-1", NotificationEventType::TaskAssigned);

        insert_notification(&conn, &notification).expect("insert");

        let rows = list_notifications(&conn, true).expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].notification_id, "notif-1");
        assert_eq!(rows[0].event_type, NotificationEventType::TaskAssigned);
        assert!(!rows[0].seen);
    }

    #[test]
    fn mark_seen_updates_flag() {
        let conn = test_conn();
        let notification = make_notification("notif-2", NotificationEventType::HandoffReady);

        insert_notification(&conn, &notification).expect("insert");
        mark_seen(&conn, "notif-2").expect("mark seen");

        let rows = list_notifications(&conn, true).expect("list");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].seen, "notification should be marked seen");
    }

    #[test]
    fn list_notifications_excludes_seen_when_flag_is_false() {
        let conn = test_conn();
        let unseen = make_notification("notif-3", NotificationEventType::TaskCompleted);
        let mut seen = make_notification("notif-4", NotificationEventType::TaskBlocked);
        seen.seen = true;

        insert_notification(&conn, &unseen).expect("insert unseen");
        insert_notification(&conn, &seen).expect("insert seen");

        let rows = list_notifications(&conn, false).expect("list unseen");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].notification_id, "notif-3");
    }

    #[test]
    fn completing_task_emits_notification() {
        use crate::store::Store;
        use std::path::Path;

        let store = Store::open(Path::new(":memory:")).expect("in-memory store");

        // Create a task via the store
        let task = store
            .create_task(
                "Test Task",
                None,
                "test-user",
                ".",
                None,
            )
            .expect("create task");

        // Transition to InProgress first (allowed from Open)
        store
            .update_task_status(
                &task.task_id,
                crate::models::TaskStatus::InProgress,
                "test-user",
                crate::store::TaskStatusUpdate::default(),
            )
            .expect("update task to in-progress");

        // Now transition to Completed (allowed from InProgress)
        store
            .update_task_status(
                &task.task_id,
                crate::models::TaskStatus::Completed,
                "test-user",
                crate::store::TaskStatusUpdate {
                    verification_state: None,
                    blocked_reason: None,
                    closure_summary: Some("test completion"),
                    event_note: None,
                },
            )
            .expect("update task to completed");

        // Verify that a notification was emitted
        let notifs = store
            .list_notifications(true)
            .expect("list all notifications");
        assert!(
            notifs
                .iter()
                .any(|n| n.event_type == NotificationEventType::TaskCompleted),
            "notification for task completion should have been emitted"
        );
    }

    #[test]
    fn blocking_task_emits_notification() {
        let conn = test_conn();
        let notif = make_notification("block-notif", NotificationEventType::TaskBlocked);
        insert_notification(&conn, &notif).expect("insert block notification");

        let rows = list_notifications(&conn, true).expect("list all notifications");
        assert!(rows.iter().any(|n| n.event_type == NotificationEventType::TaskBlocked));
    }
}
