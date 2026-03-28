use crate::models::{ApiSnapshot, TaskDetail};
use crate::store::{Store, StoreResult};

/// Builds a stable read snapshot for operator surfaces.
///
/// # Errors
///
/// Returns an error if any underlying store query fails.
pub fn snapshot(store: &Store) -> StoreResult<ApiSnapshot> {
    Ok(ApiSnapshot {
        agents: store.list_agents()?,
        tasks: store.list_tasks()?,
        handoffs: store.list_handoffs(None)?,
        evidence: store.list_all_evidence()?,
    })
}

/// Builds a task-scoped read model without exposing raw tables directly.
///
/// # Errors
///
/// Returns an error if the task does not exist or any underlying store query
/// fails.
pub fn task_detail(store: &Store, task_id: &str) -> StoreResult<TaskDetail> {
    Ok(TaskDetail {
        task: store.get_task(task_id)?,
        events: store.list_task_events(task_id)?,
        handoffs: store.list_handoffs(Some(task_id))?,
        messages: store.list_council_messages(task_id)?,
        evidence: store.list_evidence(task_id)?,
    })
}
