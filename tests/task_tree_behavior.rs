// Integration tests for parent/child task tree behavior.
//
// Covers: completion guard, auto-complete propagation, cancelled-child handling,
// and orphan handling on parent delete.

use canopy::models::{TaskRelationshipKind, TaskStatus};
use canopy::store::{Store, TaskStatusUpdate};
use rusqlite::Connection;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn open_store() -> (Store, tempfile::TempDir) {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");
    (store, temp)
}

// ---------------------------------------------------------------------------
// Test 1: completing a parent with open children is rejected
// ---------------------------------------------------------------------------

#[test]
fn complete_parent_with_open_children_is_rejected() {
    let (store, _temp) = open_store();

    let parent = store
        .create_task("Parent", None, "operator", "/tmp/proj", None)
        .expect("create parent");
    let _child = store
        .create_subtask(&parent.task_id, "Open child", None, "operator", None)
        .expect("create child");

    // Transition parent to in_progress so it can attempt to complete
    store
        .update_task_status(
            &parent.task_id,
            TaskStatus::InProgress,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect("parent in progress");

    let err = store
        .update_task_status(
            &parent.task_id,
            TaskStatus::Completed,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect_err("should reject completion with open child");

    let msg = err.to_string();
    assert!(
        msg.contains("child tasks remain open"),
        "error should mention open children: {msg}"
    );
    // The error message must include the child's task_id so MCP callers can act on it.
    assert!(
        msg.contains(&_child.task_id),
        "error should include blocking child task_id: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: completing a parent with all children completed succeeds
//         and auto-completes the parent
// ---------------------------------------------------------------------------

#[test]
fn complete_parent_after_all_children_complete_auto_completes() {
    let (store, _temp) = open_store();

    let parent = store
        .create_task("Parent", None, "operator", "/tmp/proj", None)
        .expect("create parent");
    let child = store
        .create_subtask(&parent.task_id, "Child", None, "operator", None)
        .expect("create child");

    // Complete the child
    store
        .update_task_status(
            &child.task_id,
            TaskStatus::InProgress,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect("child in progress");
    store
        .update_task_status(
            &child.task_id,
            TaskStatus::Completed,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect("child completed");

    // After the child completes, maybe_auto_complete_task_tree runs and should
    // have auto-completed the parent because all children are done.
    let parent_after = store.get_task(&parent.task_id).expect("reload parent");
    assert_eq!(
        parent_after.status,
        TaskStatus::Completed,
        "parent should be auto-completed when all children complete"
    );
}

// ---------------------------------------------------------------------------
// Test 3: completing a parent with a cancelled child succeeds
//         (cancelled children are not open — they do not block parent completion)
// ---------------------------------------------------------------------------

#[test]
fn complete_parent_with_cancelled_child_succeeds() {
    let (store, _temp) = open_store();

    let parent = store
        .create_task("Parent", None, "operator", "/tmp/proj", None)
        .expect("create parent");
    let child = store
        .create_subtask(&parent.task_id, "Cancelled child", None, "operator", None)
        .expect("create child");

    // Cancel the child
    store
        .update_task_status(
            &child.task_id,
            TaskStatus::Cancelled,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect("cancel child");

    // Parent should be auto-completed because no open children remain.
    let parent_after = store.get_task(&parent.task_id).expect("reload parent");
    assert_eq!(
        parent_after.status,
        TaskStatus::Completed,
        "parent should auto-complete once all children are non-open (cancelled counts as done)"
    );
}

// ---------------------------------------------------------------------------
// Test 4: deleting a parent orphans children cleanly — parent_task_id becomes
//         NULL and the task_relationships parent row is also removed
// ---------------------------------------------------------------------------

#[test]
fn deleting_parent_orphans_children_and_removes_relationship() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    let (parent_id, child_id) = {
        let store = Store::open(&db_path).expect("open store");
        let parent = store
            .create_task("Parent to delete", None, "operator", "/tmp/proj", None)
            .expect("create parent");
        let child = store
            .create_subtask(&parent.task_id, "Orphan child", None, "operator", None)
            .expect("create child");
        (parent.task_id, child.task_id)
    };

    // Delete the parent directly via raw SQLite (FK ON DELETE SET NULL path).
    let conn = Connection::open(&db_path).expect("open raw connection");
    conn.execute("PRAGMA foreign_keys = ON", [])
        .expect("enable foreign keys");
    conn.execute("DELETE FROM tasks WHERE task_id = ?1", [&parent_id])
        .expect("delete parent");
    drop(conn);

    // Re-open the store and verify the child is orphaned correctly.
    let store = Store::open(&db_path).expect("reopen store");

    let child = store
        .get_task(&child_id)
        .expect("child survives parent delete");
    assert_eq!(child.title, "Orphan child");
    assert!(
        child.parent_task_id.is_none(),
        "child.parent_task_id must be NULL after parent delete"
    );

    // The task_relationships row for the parent link must also be gone.
    let all_relationships = store
        .list_task_relationships(None)
        .expect("list all relationships");
    let parent_rels: Vec<_> = all_relationships
        .iter()
        .filter(|r| {
            r.kind == TaskRelationshipKind::Parent
                && (r.source_task_id == child_id || r.target_task_id == parent_id)
        })
        .collect();
    assert!(
        parent_rels.is_empty(),
        "task_relationships parent row must be removed when parent task is deleted"
    );
}
