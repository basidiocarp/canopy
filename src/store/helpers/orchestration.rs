#![allow(clippy::wildcard_imports)]

use super::*;

fn queue_name_for(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::ReviewRequired => "review",
        TaskStatus::Completed | TaskStatus::Closed | TaskStatus::Cancelled => "archive",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Open | TaskStatus::Assigned | TaskStatus::InProgress => "execution",
    }
}

fn derived_queue_status(
    status: TaskStatus,
    last_execution_action: Option<ExecutionActionKind>,
) -> TaskQueueStatus {
    match (status, last_execution_action) {
        (TaskStatus::Assigned, Some(ExecutionActionKind::PauseTask)) => TaskQueueStatus::Paused,
        (TaskStatus::Open, _) => TaskQueueStatus::Queued,
        (TaskStatus::Assigned, _) => TaskQueueStatus::Claimed,
        (TaskStatus::InProgress, _) => TaskQueueStatus::Executing,
        (TaskStatus::Blocked, _) => TaskQueueStatus::Blocked,
        (TaskStatus::ReviewRequired, _) => TaskQueueStatus::Review,
        (TaskStatus::Completed | TaskStatus::Closed, _) => TaskQueueStatus::Closed,
        (TaskStatus::Cancelled, _) => TaskQueueStatus::Cancelled,
    }
}

fn queue_lane_for(status: TaskStatus, queue_status: TaskQueueStatus) -> &'static str {
    match (status, queue_status) {
        (_, TaskQueueStatus::Paused) => "paused",
        (TaskStatus::Open, _) => "ready",
        (TaskStatus::Assigned, _) => "claimed",
        (TaskStatus::InProgress, _) => "active",
        (TaskStatus::Blocked, _) => "blocked",
        (TaskStatus::ReviewRequired, _) => "review",
        (TaskStatus::Completed | TaskStatus::Closed, _) => "closed",
        (TaskStatus::Cancelled, _) => "cancelled",
    }
}

fn last_execution_action_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<Option<ExecutionActionKind>> {
    let events = list_task_events_in_connection(conn, task_id)?;
    Ok(events
        .into_iter()
        .rev()
        .find(|event| event.event_type == TaskEventType::ExecutionUpdated)
        .and_then(|event| event.execution_action))
}

fn queue_position_for(priority: TaskPriority) -> i64 {
    match priority {
        TaskPriority::Critical => 400,
        TaskPriority::High => 300,
        TaskPriority::Medium => 200,
        TaskPriority::Low => 100,
    }
}

fn generated_execution_session_ref(
    task_id: &str,
    agent_id: Option<&str>,
    worktree_id: Option<&str>,
) -> Option<String> {
    match (agent_id, worktree_id) {
        (Some(agent_id), Some(worktree_id)) => Some(format!(
            "canopy://execution/{worktree_id}/{agent_id}/{task_id}"
        )),
        _ => None,
    }
}

pub(crate) fn load_task_queue_state_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<Option<TaskQueueStateRecord>> {
    conn.query_row(
        r"
        SELECT queue_state_id, task_id, queue_name, lane, position, status, owner_agent_id, updated_at
        FROM task_queue_states
        WHERE task_id = ?1
        ",
        [task_id],
        map_task_queue_state,
    )
    .optional()
    .map_err(StoreError::from)
}

pub(crate) fn load_task_worktree_binding_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<Option<TaskWorktreeBindingRecord>> {
    conn.query_row(
        r"
        SELECT worktree_binding_id, task_id, project_root, agent_id, worktree_id, execution_session_ref,
               status, bound_at, released_at, updated_at
        FROM task_worktree_bindings
        WHERE task_id = ?1
        ",
        [task_id],
        map_task_worktree_binding,
    )
    .optional()
    .map_err(StoreError::from)
}

pub(crate) fn load_task_review_cycle_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<Option<TaskReviewCycleRecord>> {
    conn.query_row(
        r"
        SELECT review_cycle_id, task_id, cycle_number, state, council_session_id, requested_by,
               evidence_count, decision_count, opened_at, decided_at, closed_at, updated_at
        FROM task_review_cycles
        WHERE task_id = ?1
        ORDER BY cycle_number DESC
        LIMIT 1
        ",
        [task_id],
        map_task_review_cycle,
    )
    .optional()
    .map_err(StoreError::from)
}

fn upsert_task_queue_state_in_connection(
    conn: &Connection,
    task: &Task,
) -> StoreResult<TaskQueueStateRecord> {
    let last_execution_action = last_execution_action_in_connection(conn, &task.task_id)?;
    let queue_status = derived_queue_status(task.status, last_execution_action);
    let queue_state_id = task
        .queue_state_id
        .clone()
        .or_else(|| {
            load_task_queue_state_in_connection(conn, &task.task_id)
                .ok()
                .flatten()
                .map(|record| record.queue_state_id)
        })
        .unwrap_or_else(|| Ulid::new().to_string());
    conn.execute(
        r"
        INSERT INTO task_queue_states (
            queue_state_id, task_id, queue_name, lane, position, status, owner_agent_id, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)
        ON CONFLICT(task_id) DO UPDATE SET
            queue_state_id = excluded.queue_state_id,
            queue_name = excluded.queue_name,
            lane = excluded.lane,
            position = excluded.position,
            status = excluded.status,
            owner_agent_id = excluded.owner_agent_id,
            updated_at = CURRENT_TIMESTAMP
        ",
        params![
            queue_state_id,
            task.task_id,
            queue_name_for(task.status),
            queue_lane_for(task.status, queue_status),
            queue_position_for(task.priority),
            queue_status.to_string(),
            task.owner_agent_id,
        ],
    )?;
    conn.execute(
        "UPDATE tasks SET queue_state_id = ?2 WHERE task_id = ?1",
        params![task.task_id, queue_state_id],
    )?;
    load_task_queue_state_in_connection(conn, &task.task_id)?
        .ok_or(StoreError::NotFound("task queue state"))
}

fn upsert_task_worktree_binding_in_connection(
    conn: &Connection,
    task: &Task,
) -> StoreResult<TaskWorktreeBindingRecord> {
    let existing = load_task_worktree_binding_in_connection(conn, &task.task_id)?;
    let worktree_binding_id = task
        .worktree_binding_id
        .clone()
        .or_else(|| {
            existing
                .as_ref()
                .map(|record| record.worktree_binding_id.clone())
        })
        .unwrap_or_else(|| Ulid::new().to_string());
    let (agent_id, worktree_id) = if let Some(owner_agent_id) = task.owner_agent_id.as_deref() {
        conn.query_row(
            "SELECT agent_id, worktree_id FROM agents WHERE agent_id = ?1",
            [owner_agent_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .map_or((None, None), |(agent_id, worktree_id)| {
            (Some(agent_id), Some(worktree_id))
        })
    } else {
        (None, None)
    };
    let execution_session_ref = task.execution_session_ref.clone().or_else(|| {
        generated_execution_session_ref(&task.task_id, agent_id.as_deref(), worktree_id.as_deref())
    });
    let status = if agent_id.is_some() {
        WorktreeBindingStatus::Bound
    } else if existing
        .as_ref()
        .is_some_and(|record| record.status == WorktreeBindingStatus::Bound)
    {
        WorktreeBindingStatus::Released
    } else {
        WorktreeBindingStatus::Unbound
    };
    conn.execute(
        r"
        INSERT INTO task_worktree_bindings (
            worktree_binding_id, task_id, project_root, agent_id, worktree_id, execution_session_ref,
            status, bound_at, released_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            CASE WHEN ?8 THEN CURRENT_TIMESTAMP ELSE NULL END,
            CASE WHEN ?9 THEN CURRENT_TIMESTAMP ELSE NULL END,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(task_id) DO UPDATE SET
            worktree_binding_id = excluded.worktree_binding_id,
            project_root = excluded.project_root,
            agent_id = excluded.agent_id,
            worktree_id = excluded.worktree_id,
            execution_session_ref = excluded.execution_session_ref,
            status = excluded.status,
            bound_at = CASE
                WHEN excluded.status = 'bound'
                THEN COALESCE(task_worktree_bindings.bound_at, CURRENT_TIMESTAMP)
                ELSE task_worktree_bindings.bound_at
            END,
            released_at = CASE
                WHEN excluded.status = 'released'
                THEN CURRENT_TIMESTAMP
                WHEN excluded.status = 'bound'
                THEN NULL
                ELSE task_worktree_bindings.released_at
            END,
            updated_at = CURRENT_TIMESTAMP
        ",
        params![
            worktree_binding_id,
            task.task_id,
            task.project_root,
            agent_id,
            worktree_id,
            execution_session_ref,
            status.to_string(),
            status == WorktreeBindingStatus::Bound,
            status == WorktreeBindingStatus::Released,
        ],
    )?;
    let binding = load_task_worktree_binding_in_connection(conn, &task.task_id)?
        .ok_or(StoreError::NotFound("task worktree binding"))?;
    conn.execute(
        r"
        UPDATE tasks
        SET worktree_binding_id = ?2,
            execution_session_ref = ?3
        WHERE task_id = ?1
        ",
        params![
            task.task_id,
            binding.worktree_binding_id,
            binding.execution_session_ref,
        ],
    )?;
    Ok(binding)
}

#[allow(clippy::too_many_lines)]
fn sync_task_review_cycle_in_connection(
    conn: &Connection,
    task: &Task,
) -> StoreResult<TaskReviewCycleRecord> {
    let existing = load_task_review_cycle_in_connection(conn, &task.task_id)?;
    let council_session_id = conn
        .query_row(
            "SELECT council_session_id FROM council_sessions WHERE task_id = ?1",
            [task.task_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let evidence_count = conn.query_row(
        "SELECT COUNT(*) FROM evidence_refs WHERE task_id = ?1",
        [task.task_id.as_str()],
        |row| row.get::<_, i64>(0),
    )?;
    let decision_count = conn.query_row(
        "SELECT COUNT(*) FROM council_messages WHERE task_id = ?1 AND message_type = 'decision'",
        [task.task_id.as_str()],
        |row| row.get::<_, i64>(0),
    )?;
    let events = list_task_events_in_connection(conn, &task.task_id)?;
    let context = derive_review_cycle_context(&events);
    let review_required = task.status == TaskStatus::ReviewRequired;
    let terminal = matches!(
        task.status,
        TaskStatus::Completed | TaskStatus::Closed | TaskStatus::Cancelled
    );

    let mut review_cycle_id = existing.as_ref().map_or_else(
        || Ulid::new().to_string(),
        |record| record.review_cycle_id.clone(),
    );
    let mut cycle_number = existing.as_ref().map_or(1, |record| record.cycle_number);

    if review_required
        && existing
            .as_ref()
            .is_some_and(|record| record.state == ReviewCycleState::Closed)
    {
        cycle_number += 1;
        review_cycle_id = Ulid::new().to_string();
    }

    let next_state = if terminal {
        ReviewCycleState::Closed
    } else if review_required {
        if context.has_council_decision {
            ReviewCycleState::DecisionReady
        } else if context.has_evidence || context.has_council_message {
            ReviewCycleState::InReview
        } else {
            ReviewCycleState::Pending
        }
    } else if existing
        .as_ref()
        .is_some_and(|record| record.state == ReviewCycleState::Closed)
    {
        ReviewCycleState::Closed
    } else {
        ReviewCycleState::Inactive
    };

    conn.execute(
        r"
        INSERT INTO task_review_cycles (
            review_cycle_id, task_id, cycle_number, state, council_session_id, requested_by,
            evidence_count, decision_count, opened_at, decided_at, closed_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
            CASE WHEN ?9 THEN CURRENT_TIMESTAMP ELSE NULL END,
            CASE WHEN ?10 THEN CURRENT_TIMESTAMP ELSE NULL END,
            CASE WHEN ?11 THEN CURRENT_TIMESTAMP ELSE NULL END,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(task_id, cycle_number) DO UPDATE SET
            review_cycle_id = excluded.review_cycle_id,
            state = excluded.state,
            council_session_id = excluded.council_session_id,
            requested_by = COALESCE(task_review_cycles.requested_by, excluded.requested_by),
            evidence_count = excluded.evidence_count,
            decision_count = excluded.decision_count,
            opened_at = CASE
                WHEN excluded.state IN ('pending', 'in_review', 'decision_ready')
                THEN COALESCE(task_review_cycles.opened_at, CURRENT_TIMESTAMP)
                ELSE task_review_cycles.opened_at
            END,
            decided_at = CASE
                WHEN excluded.state = 'decision_ready'
                THEN COALESCE(task_review_cycles.decided_at, CURRENT_TIMESTAMP)
                ELSE task_review_cycles.decided_at
            END,
            closed_at = CASE
                WHEN excluded.state = 'closed'
                THEN COALESCE(task_review_cycles.closed_at, CURRENT_TIMESTAMP)
                ELSE task_review_cycles.closed_at
            END,
            updated_at = CURRENT_TIMESTAMP
        ",
        params![
            review_cycle_id,
            task.task_id,
            cycle_number,
            next_state.to_string(),
            council_session_id,
            review_required.then_some(task.requested_by.as_str()),
            evidence_count,
            decision_count,
            matches!(
                next_state,
                ReviewCycleState::Pending
                    | ReviewCycleState::InReview
                    | ReviewCycleState::DecisionReady
            ),
            next_state == ReviewCycleState::DecisionReady,
            next_state == ReviewCycleState::Closed,
        ],
    )?;
    let review_cycle = load_task_review_cycle_in_connection(conn, &task.task_id)?
        .ok_or(StoreError::NotFound("task review cycle"))?;
    conn.execute(
        "UPDATE tasks SET review_cycle_id = ?2 WHERE task_id = ?1",
        params![task.task_id, review_cycle.review_cycle_id],
    )?;
    Ok(review_cycle)
}

pub(crate) fn sync_task_workflow_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<()> {
    let task = get_task_in_connection(conn, task_id)?;
    let queue_state = upsert_task_queue_state_in_connection(conn, &task)?;
    let worktree_binding = upsert_task_worktree_binding_in_connection(conn, &task)?;
    let review_cycle = sync_task_review_cycle_in_connection(conn, &task)?;
    conn.execute(
        r"
        UPDATE tasks
        SET queue_state_id = ?2,
            worktree_binding_id = ?3,
            execution_session_ref = ?4,
            review_cycle_id = ?5
        WHERE task_id = ?1
        ",
        params![
            task_id,
            queue_state.queue_state_id,
            worktree_binding.worktree_binding_id,
            worktree_binding.execution_session_ref,
            review_cycle.review_cycle_id,
        ],
    )?;
    Ok(())
}
