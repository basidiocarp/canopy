use crate::models::{
    ApiSnapshot, Task, TaskDetail, TaskSort, TaskStatus, TaskView, VerificationState,
};
use crate::store::{Store, StoreResult};

#[derive(Debug, Clone, Copy)]
pub struct SnapshotOptions<'a> {
    pub project_root: Option<&'a str>,
    pub sort: TaskSort,
    pub view: TaskView,
}

impl Default for SnapshotOptions<'_> {
    fn default() -> Self {
        Self {
            project_root: None,
            sort: TaskSort::Status,
            view: TaskView::All,
        }
    }
}

/// Builds a stable read snapshot for operator surfaces.
///
/// # Errors
///
/// Returns an error if any underlying store query fails.
pub fn snapshot(store: &Store, options: SnapshotOptions<'_>) -> StoreResult<ApiSnapshot> {
    let mut agents = store.list_agents()?;
    if let Some(project_root) = options.project_root {
        agents.retain(|agent| agent.project_root == project_root);
    }

    let handoffs = store.list_handoffs(None)?;
    let mut tasks = store.list_tasks()?;

    if let Some(project_root) = options.project_root {
        tasks.retain(|task| task.project_root == project_root);
    }

    let open_handoff_task_ids: std::collections::HashSet<_> = handoffs
        .iter()
        .filter(|handoff| handoff.status.to_string() == "open")
        .map(|handoff| handoff.task_id.clone())
        .collect();

    tasks.retain(|task| matches_view(task, &open_handoff_task_ids, options.view));
    sort_tasks(&mut tasks, options.sort);

    let task_ids: std::collections::HashSet<_> =
        tasks.iter().map(|task| task.task_id.clone()).collect();
    let agent_ids: std::collections::HashSet<_> =
        agents.iter().map(|agent| agent.agent_id.clone()).collect();
    let heartbeats = store
        .list_all_agent_heartbeats()?
        .into_iter()
        .filter(|heartbeat| {
            agent_ids.contains(&heartbeat.agent_id)
                && heartbeat.current_task_id.as_ref().is_none_or(|task_id| task_ids.contains(task_id))
                && heartbeat.related_task_id.as_ref().is_none_or(|task_id| task_ids.contains(task_id))
        })
        .take(50)
        .collect();

    Ok(ApiSnapshot {
        agents,
        heartbeats,
        tasks,
        handoffs: handoffs
            .into_iter()
            .filter(|handoff| task_ids.contains(&handoff.task_id))
            .collect(),
        evidence: store
            .list_all_evidence()?
            .into_iter()
            .filter(|evidence| task_ids.contains(&evidence.task_id))
            .collect(),
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
        heartbeats: store.list_task_heartbeats(task_id, 25)?,
        handoffs: store.list_handoffs(Some(task_id))?,
        messages: store.list_council_messages(task_id)?,
        evidence: store.list_evidence(task_id)?,
    })
}

fn matches_view(
    task: &Task,
    open_handoff_task_ids: &std::collections::HashSet<String>,
    view: TaskView,
) -> bool {
    match view {
        TaskView::All => true,
        TaskView::Active => matches!(
            task.status,
            TaskStatus::Open | TaskStatus::Assigned | TaskStatus::InProgress
        ),
        TaskView::Blocked => {
            task.status == TaskStatus::Blocked
                || task.verification_state == VerificationState::Failed
        }
        TaskView::Review => {
            task.status == TaskStatus::ReviewRequired
                || task.verification_state == VerificationState::Pending
        }
        TaskView::Handoffs => open_handoff_task_ids.contains(&task.task_id),
    }
}

fn sort_tasks(tasks: &mut [Task], sort: TaskSort) {
    tasks.sort_by(|left, right| match sort {
        TaskSort::Title => left.title.cmp(&right.title),
        TaskSort::UpdatedAt => right.updated_at.cmp(&left.updated_at),
        TaskSort::CreatedAt => right.created_at.cmp(&left.created_at),
        TaskSort::Verification => verification_rank(left.verification_state)
            .cmp(&verification_rank(right.verification_state))
            .then_with(|| left.title.cmp(&right.title)),
        TaskSort::Status => status_rank(left.status)
            .cmp(&status_rank(right.status))
            .then_with(|| left.title.cmp(&right.title)),
    });
}

fn status_rank(status: TaskStatus) -> u8 {
    match status {
        TaskStatus::InProgress => 0,
        TaskStatus::ReviewRequired => 1,
        TaskStatus::Blocked => 2,
        TaskStatus::Assigned => 3,
        TaskStatus::Open => 4,
        TaskStatus::Completed => 5,
        TaskStatus::Closed => 6,
        TaskStatus::Cancelled => 7,
    }
}

fn verification_rank(state: VerificationState) -> u8 {
    match state {
        VerificationState::Failed => 0,
        VerificationState::Pending => 1,
        VerificationState::Unknown => 2,
        VerificationState::Passed => 3,
    }
}
