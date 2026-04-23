#![allow(clippy::wildcard_imports)]

use super::*;

#[allow(dead_code)]
pub(crate) fn has_open_child_tasks_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<bool> {
    let mut stmt = conn.prepare(
        r"
        WITH RECURSIVE descendants(task_id) AS (
            SELECT task_id FROM tasks WHERE parent_task_id = ?1
            UNION ALL
            SELECT t.task_id FROM tasks t
            INNER JOIN descendants d ON t.parent_task_id = d.task_id
        )
        SELECT tasks.status
        FROM descendants
        INNER JOIN tasks ON tasks.task_id = descendants.task_id
        ",
    )?;
    let rows = stmt.query_map([task_id], |row| row.get::<_, String>(0))?;
    for row in rows {
        let status = parse_enum_value::<TaskStatus>(&row?, 0)?;
        if is_open_task_status(status) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Returns (`task_id`, title, status) for all direct open children of a task.
///
/// Only direct children are returned (not recursive descendants). This is used
/// to produce actionable error messages when a parent cannot complete.
pub(crate) fn list_open_children_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<Vec<(String, String, TaskStatus)>> {
    let mut stmt = conn.prepare(
        r"
        SELECT task_id, title, status
        FROM tasks
        WHERE parent_task_id = ?1
        ORDER BY created_at ASC, task_id ASC
        ",
    )?;
    let rows = stmt.query_map([task_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut open = Vec::new();
    for row in rows {
        let (id, title, status_str) = row?;
        let status = parse_enum_value::<TaskStatus>(&status_str, 2)?;
        if is_open_task_status(status) {
            open.push((id, title, status));
        }
    }
    Ok(open)
}

pub(crate) fn has_passing_script_verification_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<bool> {
    let mut stmt = conn.prepare(
        r"
        SELECT summary
        FROM evidence_refs
        WHERE task_id = ?1
          AND source_kind = ?2
        ORDER BY rowid DESC
        ",
    )?;
    let rows = stmt.query_map(
        params![task_id, EvidenceSourceKind::ScriptVerification.to_string()],
        |row| row.get::<_, Option<String>>(0),
    )?;
    for row in rows {
        let Some(summary) = row? else {
            continue;
        };
        // The evidence_refs table stores only a free-text `summary` column;
        // there is no structured boolean or enum field for verification outcome.
        // Until the schema gains a dedicated outcome field, substring matching
        // on the canonical phrase emitted by the script-verification tool is
        // the only available signal.
        if summary.contains("script verification passed") {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn maybe_auto_complete_task_tree_in_connection(
    conn: &Connection,
    task_id: &str,
    changed_by: &str,
) -> StoreResult<()> {
    // Walk the ancestor chain with a recursive CTE so the full list of
    // ancestors is fetched in a single query rather than one hop at a time.
    // Start from the task itself and follow parent_task_id upward.
    let mut stmt = conn.prepare(
        r"
        WITH RECURSIVE ancestors(task_id, parent_task_id) AS (
            SELECT task_id, parent_task_id FROM tasks WHERE task_id = ?1
            UNION ALL
            SELECT t.task_id, t.parent_task_id FROM tasks t
            INNER JOIN ancestors a ON t.task_id = a.parent_task_id
        )
        SELECT task_id FROM ancestors
        ",
    )?;
    let ancestor_ids: Vec<String> = stmt
        .query_map([task_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)?;

    for candidate_task_id in ancestor_ids {
        maybe_auto_complete_task_in_connection(conn, &candidate_task_id, changed_by)?;
    }
    Ok(())
}

fn maybe_auto_complete_task_in_connection(
    conn: &Connection,
    task_id: &str,
    changed_by: &str,
) -> StoreResult<()> {
    let task = get_task_in_connection(conn, task_id)?;
    if matches!(
        task.status,
        TaskStatus::Completed | TaskStatus::Closed | TaskStatus::Cancelled
    ) {
        return Ok(());
    }

    let mut stmt = conn.prepare(
        r"
        SELECT tasks.status
        FROM tasks
        WHERE tasks.parent_task_id = ?1
        ",
    )?;
    let rows = stmt.query_map([task_id], |row| row.get::<_, String>(0))?;
    let mut has_children = false;
    for row in rows {
        has_children = true;
        let status = parse_enum_value::<TaskStatus>(&row?, 0)?;
        if is_open_task_status(status) {
            return Ok(());
        }
    }
    if !has_children {
        return Ok(());
    }

    if task.verification_required
        && (task.verification_state != VerificationState::Passed
            || !has_passing_script_verification_in_connection(conn, task_id)?)
    {
        return Ok(());
    }

    conn.execute(
        r"
        UPDATE tasks
        SET status = ?2,
            blocked_reason = NULL,
            closed_by = ?3,
            closure_summary = ?4,
            closed_at = CURRENT_TIMESTAMP,
            updated_at = CURRENT_TIMESTAMP
        WHERE task_id = ?1
        ",
        params![
            task_id,
            TaskStatus::Completed.to_string(),
            changed_by,
            "all child tasks completed",
        ],
    )?;
    sync_owner_for_task_status(conn, task_id, TaskStatus::Completed)?;
    let updated = get_task_in_connection(conn, task_id)?;
    record_task_event_in_connection(
        conn,
        &TaskEventWrite {
            task_id,
            event_type: TaskEventType::StatusChanged,
            actor: changed_by,
            from_status: Some(task.status),
            to_status: TaskStatus::Completed,
            verification_state: Some(updated.verification_state),
            owner_agent_id: updated.owner_agent_id.as_deref(),
            execution_action: None,
            execution_duration_seconds: None,
            note: Some(
                "all child tasks completed; note=auto_parent_completion=all_children_complete",
            ),
        },
    )?;
    Ok(())
}

pub(crate) fn sync_owner_for_task_status(
    conn: &Connection,
    task_id: &str,
    status: TaskStatus,
) -> StoreResult<()> {
    let owner_agent_id = conn
        .query_row(
            "SELECT owner_agent_id FROM tasks WHERE task_id = ?1",
            [task_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();

    let Some(owner_agent_id) = owner_agent_id else {
        return Ok(());
    };

    let (agent_status, current_task_id): (AgentStatus, Option<&str>) = match status {
        TaskStatus::Assigned => (AgentStatus::Assigned, Some(task_id)),
        TaskStatus::InProgress => (AgentStatus::InProgress, Some(task_id)),
        TaskStatus::Blocked => (AgentStatus::Blocked, Some(task_id)),
        TaskStatus::ReviewRequired => (AgentStatus::ReviewRequired, Some(task_id)),
        TaskStatus::Completed | TaskStatus::Closed | TaskStatus::Cancelled => {
            (AgentStatus::Idle, None)
        }
        TaskStatus::Open => (AgentStatus::Idle, None),
    };

    conn.execute(
        r"
        UPDATE agents
        SET status = ?2,
            current_task_id = ?3,
            heartbeat_at = CURRENT_TIMESTAMP
        WHERE agent_id = ?1
        ",
        params![owner_agent_id, agent_status.to_string(), current_task_id],
    )?;
    record_agent_heartbeat_in_connection(
        conn,
        &AgentHeartbeatWrite {
            agent_id: &owner_agent_id,
            status: agent_status,
            current_task_id,
            related_task_id: Some(task_id),
            source: AgentHeartbeatSource::TaskSync,
        },
    )?;

    Ok(())
}

pub(crate) fn list_handoffs_for_task_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<Vec<Handoff>> {
    let mut stmt = conn.prepare(
        r"
        SELECT handoff_id, task_id, from_agent_id, to_agent_id, handoff_type,
               summary, requested_action, goal, next_steps, stop_reason, due_at, expires_at, status, created_at, updated_at, resolved_at
        FROM handoffs
        WHERE task_id = ?1
        ORDER BY rowid
        ",
    )?;
    let rows = stmt.query_map([task_id], map_handoff)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

pub(crate) fn has_unresolved_review_handoffs_in_connection(
    conn: &Connection,
    task_id: &str,
    handoff_types: &[HandoffType],
) -> StoreResult<bool> {
    let handoffs = list_handoffs_for_task_in_connection(conn, task_id)?;
    for handoff in handoffs {
        if !handoff_types.contains(&handoff.handoff_type) {
            continue;
        }
        let unresolved = match handoff.status {
            HandoffStatus::Open => !handoff_is_expired(&handoff)?,
            HandoffStatus::Accepted => true,
            HandoffStatus::Rejected
            | HandoffStatus::Expired
            | HandoffStatus::Cancelled
            | HandoffStatus::Completed => false,
        };
        if unresolved {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn has_open_follow_up_children_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<bool> {
    let mut stmt = conn.prepare(
        r"
        SELECT 1
        FROM task_relationships
        INNER JOIN tasks ON tasks.task_id = task_relationships.target_task_id
        WHERE task_relationships.kind = 'follow_up'
          AND task_relationships.source_task_id = ?1
          AND tasks.status IN ('open', 'assigned', 'in_progress', 'blocked', 'review_required')
        LIMIT 1
        ",
    )?;
    Ok(stmt.exists([task_id])?)
}

pub(crate) fn has_active_blockers_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<bool> {
    let mut stmt = conn.prepare(
        r"
        SELECT 1
        FROM task_relationships
        INNER JOIN tasks ON tasks.task_id = task_relationships.source_task_id
        WHERE task_relationships.kind = 'blocks'
          AND task_relationships.target_task_id = ?1
          AND tasks.status IN ('open', 'assigned', 'in_progress', 'blocked', 'review_required')
        LIMIT 1
        ",
    )?;
    Ok(stmt.exists([task_id])?)
}

pub(crate) fn compute_open_execution_duration_seconds(
    conn: &Connection,
    task_id: &str,
    now: OffsetDateTime,
) -> StoreResult<Option<i64>> {
    let events = list_task_events_in_connection(conn, task_id)?;
    let mut last_start: Option<TaskEvent> = None;
    for event in events {
        if event.event_type != TaskEventType::ExecutionUpdated {
            continue;
        }
        match event.execution_action {
            Some(ExecutionActionKind::StartTask | ExecutionActionKind::ResumeTask) => {
                last_start = Some(event);
            }
            Some(
                ExecutionActionKind::PauseTask
                | ExecutionActionKind::YieldTask
                | ExecutionActionKind::CompleteTask,
            ) => {
                last_start = None;
            }
            Some(ExecutionActionKind::ClaimTask) | None => {}
        }
    }

    let Some(start_event) = last_start else {
        return Ok(None);
    };
    let started_at = parse_database_timestamp(&start_event.created_at)?;
    let elapsed = (now - started_at).num_seconds();
    Ok(Some(elapsed.max(0)))
}

pub(crate) fn task_has_prior_execution_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<bool> {
    let events = list_task_events_in_connection(conn, task_id)?;
    Ok(events.into_iter().any(|event| {
        event.event_type == TaskEventType::ExecutionUpdated
            && matches!(
                event.execution_action,
                Some(ExecutionActionKind::StartTask | ExecutionActionKind::ResumeTask)
            )
    }))
}

pub(crate) fn release_agent_current_task_in_connection(
    conn: &Connection,
    agent_id: &str,
    task_id: &str,
) -> StoreResult<()> {
    conn.execute(
        r"
        UPDATE agents
        SET current_task_id = NULL, status = 'idle', heartbeat_at = CURRENT_TIMESTAMP
        WHERE agent_id = ?1 AND current_task_id = ?2
        ",
        params![agent_id, task_id],
    )?;
    record_agent_heartbeat_in_connection(
        conn,
        &AgentHeartbeatWrite {
            agent_id,
            status: AgentStatus::Idle,
            current_task_id: None,
            related_task_id: Some(task_id),
            source: AgentHeartbeatSource::TaskSync,
        },
    )?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(crate) fn assign_task_in_connection(
    conn: &Connection,
    task_id: &str,
    assigned_to: &str,
    assigned_by: &str,
    reason: Option<&str>,
) -> StoreResult<()> {
    let assignee_current_task = conn
        .query_row(
            "SELECT current_task_id FROM agents WHERE agent_id = ?1",
            [assigned_to],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    if assignee_current_task
        .as_deref()
        .is_some_and(|current_task_id| current_task_id != task_id)
    {
        return Err(StoreError::Validation(
            "assigned agent already owns another active task".to_string(),
        ));
    }
    let assignee_role_and_capabilities = conn
        .query_row(
            "SELECT role, capabilities FROM agents WHERE agent_id = ?1",
            [assigned_to],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?
        .ok_or(StoreError::NotFound("agent"))?;
    let assignee_role = assignee_role_and_capabilities
        .0
        .map(|value| parse_enum_value::<AgentRole>(&value, 0))
        .transpose()?;
    let assignee_capabilities = assignee_role_and_capabilities
        .1
        .map_or_else(Vec::new, |json| parse_capabilities(&json));
    let from_status = conn
        .query_row(
            "SELECT status FROM tasks WHERE task_id = ?1",
            [task_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| parse_enum_value::<TaskStatus>(&value, 0))
        .transpose()?;
    let previous_owner = conn
        .query_row(
            "SELECT owner_agent_id FROM tasks WHERE task_id = ?1",
            [task_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .ok_or(StoreError::NotFound("task"))?;
    let required_role_and_capabilities = conn
        .query_row(
            "SELECT required_role, required_capabilities FROM tasks WHERE task_id = ?1",
            [task_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?
        .ok_or(StoreError::NotFound("task"))?;
    let required_role = required_role_and_capabilities
        .0
        .map(|value| parse_enum_value::<AgentRole>(&value, 0))
        .transpose()?;
    let required_capabilities = required_role_and_capabilities
        .1
        .map_or_else(Vec::new, |json| parse_capabilities(&json));

    if let (Some(required_role), Some(assignee_role)) = (required_role, assignee_role)
        && required_role != assignee_role
    {
        return Err(StoreError::Validation(format!(
            "task requires {required_role} role, agent has {assignee_role}"
        )));
    }
    if !capabilities_match(&assignee_capabilities, &required_capabilities) {
        let missing = required_capabilities
            .iter()
            .filter(|required_capability| !assignee_capabilities.contains(required_capability))
            .cloned()
            .collect::<Vec<_>>();
        return Err(StoreError::Validation(format!(
            "agent missing capabilities: {}",
            missing.join(", ")
        )));
    }

    conn.execute(
        r"
        UPDATE tasks
        SET owner_agent_id = ?2,
            status = 'assigned',
            updated_at = CURRENT_TIMESTAMP
        WHERE task_id = ?1
        ",
        params![task_id, assigned_to],
    )?;
    conn.execute(
        r"
        INSERT INTO task_assignments (assignment_id, task_id, assigned_to, assigned_by, reason)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ",
        params![
            Ulid::new().to_string(),
            task_id,
            assigned_to,
            assigned_by,
            reason
        ],
    )?;
    let event_type = if previous_owner
        .as_deref()
        .is_some_and(|owner| owner != assigned_to)
    {
        TaskEventType::OwnershipTransferred
    } else {
        TaskEventType::Assigned
    };
    let owner_change_note = match previous_owner.as_deref() {
        Some(previous_owner) if previous_owner != assigned_to => {
            format!("owner:{previous_owner}->{assigned_to}")
        }
        Some(previous_owner) => format!("owner:{previous_owner}->{assigned_to}"),
        None => format!("owner:none->{assigned_to}"),
    };
    let note = reason.map_or(owner_change_note.clone(), |reason| {
        format!("{owner_change_note}; note={reason}")
    });
    record_task_event_in_connection(
        conn,
        &TaskEventWrite {
            task_id,
            event_type,
            actor: assigned_by,
            from_status,
            to_status: TaskStatus::Assigned,
            verification_state: None,
            owner_agent_id: Some(assigned_to),
            execution_action: None,
            execution_duration_seconds: None,
            note: Some(note.as_str()),
        },
    )?;

    if let Some(previous_owner) = previous_owner.filter(|owner| owner != assigned_to) {
        conn.execute(
            r"
            UPDATE agents
            SET current_task_id = NULL, status = 'idle', heartbeat_at = CURRENT_TIMESTAMP
            WHERE agent_id = ?1 AND current_task_id = ?2
            ",
            params![previous_owner, task_id],
        )?;
        record_agent_heartbeat_in_connection(
            conn,
            &AgentHeartbeatWrite {
                agent_id: &previous_owner,
                status: AgentStatus::Idle,
                current_task_id: None,
                related_task_id: Some(task_id),
                source: AgentHeartbeatSource::TaskSync,
            },
        )?;
    }

    conn.execute(
        r"
        UPDATE agents
        SET current_task_id = ?2, status = 'assigned', heartbeat_at = CURRENT_TIMESTAMP
        WHERE agent_id = ?1
        ",
        params![assigned_to, task_id],
    )?;
    record_agent_heartbeat_in_connection(
        conn,
        &AgentHeartbeatWrite {
            agent_id: assigned_to,
            status: AgentStatus::Assigned,
            current_task_id: Some(task_id),
            related_task_id: Some(task_id),
            source: AgentHeartbeatSource::TaskSync,
        },
    )?;

    sync_task_workflow_in_connection(conn, task_id)?;

    Ok(())
}

pub(crate) fn build_execution_note(
    changed_by: &str,
    acting_agent_id: &str,
    note: Option<&str>,
) -> Option<String> {
    let mut notes = Vec::new();
    if changed_by != acting_agent_id {
        notes.push(format!("changed_by={changed_by}"));
    }
    if let Some(note) = note.filter(|value| !value.trim().is_empty()) {
        notes.push(format!("note={note}"));
    }
    (!notes.is_empty()).then(|| notes.join("; "))
}

pub(crate) fn validate_execution_actor<'a>(
    task: &Task,
    acting_agent_id: Option<&'a str>,
    action_name: &str,
) -> StoreResult<&'a str> {
    let acting_agent_id = acting_agent_id.ok_or_else(|| {
        StoreError::Validation(format!("{action_name} requires an acting_agent_id"))
    })?;
    if task.owner_agent_id.as_deref() != Some(acting_agent_id) {
        return Err(StoreError::Validation(format!(
            "{action_name} requires the acting agent to own the task"
        )));
    }
    Ok(acting_agent_id)
}

pub(crate) fn check_file_conflicts_in_connection(
    conn: &Connection,
    files: &[String],
    worktree_id: &str,
    exclude_agent_id: Option<&str>,
) -> StoreResult<Vec<FileLock>> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let mut param_offset = 2;
    if exclude_agent_id.is_some() {
        param_offset = 3;
    }
    let placeholders: Vec<String> = (0..files.len())
        .map(|i| format!("?{}", i + param_offset))
        .collect();
    let placeholders_str = placeholders.join(", ");
    let mut sql = format!(
        r"
        SELECT lock_id, task_id, agent_id, file_path, worktree_id, locked_at, released_at
        FROM file_locks
        WHERE released_at IS NULL
          AND worktree_id = ?1
          AND file_path IN ({placeholders_str})
        "
    );
    if exclude_agent_id.is_some() {
        sql.push_str(" AND agent_id != ?2");
    }
    sql.push_str(" ORDER BY locked_at");

    let mut stmt = conn.prepare(&sql)?;
    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params_vec.push(Box::new(worktree_id.to_string()));
    if let Some(agent_id) = exclude_agent_id {
        params_vec.push(Box::new(agent_id.to_string()));
    }
    for file in files {
        params_vec.push(Box::new(file.clone()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(AsRef::as_ref).collect();

    let rows = stmt.query_map(&*param_refs, map_file_lock)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EvidenceNavigation<'a> {
    pub session_id: Option<&'a str>,
    pub memory_query: Option<&'a str>,
    pub symbol: Option<&'a str>,
    pub file: Option<&'a str>,
}

pub(crate) fn normalize_evidence_navigation<'a>(
    source_kind: EvidenceSourceKind,
    source_ref: &'a str,
    session_id: Option<&'a str>,
    memory_query: Option<&'a str>,
    symbol: Option<&'a str>,
    file: Option<&'a str>,
) -> EvidenceNavigation<'a> {
    match source_kind {
        EvidenceSourceKind::HyphaeSession => EvidenceNavigation {
            session_id: session_id.or(Some(source_ref)),
            memory_query,
            symbol,
            file,
        },
        EvidenceSourceKind::HyphaeRecall
        | EvidenceSourceKind::HyphaeOutcome
        | EvidenceSourceKind::CortinaEvent
        | EvidenceSourceKind::ManualNote
        | EvidenceSourceKind::RhizomeImpact
        | EvidenceSourceKind::RhizomeExport
        | EvidenceSourceKind::ScriptVerification
        | EvidenceSourceKind::MyceliumCommand
        | EvidenceSourceKind::MyceliumExplain => EvidenceNavigation {
            session_id,
            memory_query,
            symbol,
            file,
        },
    }
}
