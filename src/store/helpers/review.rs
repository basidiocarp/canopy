#![allow(clippy::wildcard_imports)]

use super::*;

pub(crate) fn maybe_create_auto_review_subtasks_in_connection(
    conn: &Connection,
    handoff: &Handoff,
    status: HandoffStatus,
    actor: &str,
) -> StoreResult<()> {
    if status != HandoffStatus::Completed {
        return Ok(());
    }
    if !matches!(
        handoff.handoff_type,
        HandoffType::TransferOwnership | HandoffType::RequestReview
    ) {
        return Ok(());
    }

    let task = get_task_in_connection(conn, &handoff.task_id)?;
    if !task.auto_review
        || task_priority_rank(task.priority)
            < task_priority_rank(super::super::AUTO_REVIEW_MIN_PRIORITY)
    {
        return Ok(());
    }

    let parent_id = get_parent_id_in_connection(conn, &task.task_id)?;
    let review_task_ids =
        create_review_subtasks_in_connection(conn, &task, parent_id.as_deref(), actor)?;
    if review_task_ids.is_empty() {
        return Ok(());
    }

    let note = format!(
        "action=auto_review_subtasks; review_task_ids={}",
        review_task_ids.join(",")
    );
    record_task_event_in_connection(
        conn,
        &TaskEventWrite {
            task_id: &task.task_id,
            event_type: TaskEventType::RelationshipUpdated,
            actor,
            from_status: Some(task.status),
            to_status: task.status,
            verification_state: Some(task.verification_state),
            owner_agent_id: task.owner_agent_id.as_deref(),
            execution_action: None,
            execution_duration_seconds: None,
            note: Some(note.as_str()),
        },
    )?;

    Ok(())
}

fn create_review_subtasks_in_connection(
    conn: &Connection,
    implementation_task: &Task,
    parent_id: Option<&str>,
    actor: &str,
) -> StoreResult<Vec<String>> {
    let review_priority = lower_review_priority(implementation_task.priority);
    let mut review_task_ids = Vec::with_capacity(super::super::AUTO_REVIEW_SUBTASKS.len());

    for (title, instruction) in super::super::AUTO_REVIEW_SUBTASKS {
        let description = format!(
            "{instruction}. Review source task {} ({}) in project {}.",
            implementation_task.task_id,
            implementation_task.title,
            implementation_task.project_root
        );
        let review_task = create_task_in_connection(
            conn,
            title,
            Some(description.as_str()),
            actor,
            &implementation_task.project_root,
            &TaskCreationOptions {
                required_role: Some(AgentRole::Validator),
                required_capabilities: vec!["code-review".to_string()],
                auto_review: false,
                verification_required: false,
                scope: Vec::new(),
            },
        )?;
        set_task_priority_in_connection(conn, &review_task.task_id, review_priority)?;
        if let Some(parent_id) = parent_id {
            record_parent_relationship_in_connection(conn, &review_task.task_id, parent_id, actor)?;
        }
        review_task_ids.push(review_task.task_id);
    }

    Ok(review_task_ids)
}

fn set_task_priority_in_connection(
    conn: &Connection,
    task_id: &str,
    priority: TaskPriority,
) -> StoreResult<()> {
    conn.execute(
        r"
        UPDATE tasks
        SET priority = ?2,
            updated_at = CURRENT_TIMESTAMP
        WHERE task_id = ?1
        ",
        params![task_id, priority.to_string()],
    )?;
    Ok(())
}

fn get_parent_id_in_connection(conn: &Connection, task_id: &str) -> StoreResult<Option<String>> {
    conn.query_row(
        r"
        SELECT parent_task_id
        FROM tasks
        WHERE task_id = ?1
        ",
        [task_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map(Option::flatten)
    .map_err(StoreError::from)
}

fn task_priority_rank(priority: TaskPriority) -> u8 {
    match priority {
        TaskPriority::Low => 0,
        TaskPriority::Medium => 1,
        TaskPriority::High => 2,
        TaskPriority::Critical => 3,
    }
}

fn lower_review_priority(priority: TaskPriority) -> TaskPriority {
    match priority {
        TaskPriority::Critical => TaskPriority::High,
        TaskPriority::High => TaskPriority::Medium,
        TaskPriority::Medium | TaskPriority::Low => TaskPriority::Low,
    }
}
