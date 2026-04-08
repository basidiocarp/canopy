#![allow(clippy::wildcard_imports)]

use super::*;

pub(crate) fn get_task_in_connection(conn: &Connection, task_id: &str) -> StoreResult<Task> {
    conn.query_row(
        r"
        SELECT task_id, title, description, requested_by, project_root, required_role,
               required_capabilities, auto_review, verification_required, status, verification_state, priority, severity, owner_agent_id, owner_note,
               acknowledged_by, acknowledged_at, blocked_reason, verified_by,
               verified_at, closed_by, closure_summary, closed_at, due_at, review_due_at,
               parent_task_id, scope, created_at, updated_at
        FROM tasks
        WHERE task_id = ?1
        ",
        [task_id],
        map_task,
    )
    .optional()?
    .ok_or(StoreError::NotFound("task"))
}

pub(crate) fn get_agent_in_connection(
    conn: &Connection,
    agent_id: &str,
) -> StoreResult<AgentRegistration> {
    conn.query_row(
        r"
        SELECT agent_id, host_id, host_type, host_instance, model,
               project_root, worktree_id, role, capabilities, status, current_task_id, heartbeat_at
        FROM agents
        WHERE agent_id = ?1
        ",
        [agent_id],
        map_agent,
    )
    .optional()?
    .ok_or(StoreError::NotFound("agent"))
}

pub(crate) fn get_handoff_in_connection(
    conn: &Connection,
    handoff_id: &str,
) -> StoreResult<Handoff> {
    conn.query_row(
        r"
        SELECT handoff_id, task_id, from_agent_id, to_agent_id, handoff_type,
               summary, requested_action, due_at, expires_at, status, created_at, updated_at, resolved_at
        FROM handoffs
        WHERE handoff_id = ?1
        ",
        [handoff_id],
        map_handoff,
    )
    .optional()?
    .ok_or(StoreError::NotFound("handoff"))
}

pub(crate) fn touch_task_in_connection(conn: &Connection, task_id: &str) -> StoreResult<()> {
    conn.execute(
        "UPDATE tasks SET updated_at = CURRENT_TIMESTAMP WHERE task_id = ?1",
        [task_id],
    )?;
    Ok(())
}

pub(crate) fn record_task_event_in_connection(
    conn: &Connection,
    event: &TaskEventWrite<'_>,
) -> StoreResult<()> {
    conn.execute(
        r"
        INSERT INTO task_events (
            event_id, task_id, event_type, actor, from_status, to_status,
            verification_state, owner_agent_id, execution_action,
            execution_duration_seconds, note
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ",
        params![
            Ulid::new().to_string(),
            event.task_id,
            event.event_type.to_string(),
            event.actor,
            event.from_status.map(|value| value.to_string()),
            event.to_status.to_string(),
            event.verification_state.map(|value| value.to_string()),
            event.owner_agent_id,
            event.execution_action.map(|value| value.to_string()),
            event.execution_duration_seconds,
            event.note,
        ],
    )?;
    Ok(())
}

pub(crate) fn record_agent_heartbeat_in_connection(
    conn: &Connection,
    heartbeat: &AgentHeartbeatWrite<'_>,
) -> StoreResult<()> {
    conn.execute(
        r"
        INSERT INTO agent_heartbeat_events (
            heartbeat_id, agent_id, status, current_task_id, related_task_id, source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ",
        params![
            Ulid::new().to_string(),
            heartbeat.agent_id,
            heartbeat.status.to_string(),
            heartbeat.current_task_id,
            heartbeat.related_task_id,
            heartbeat.source.to_string(),
        ],
    )?;
    Ok(())
}

pub(crate) fn list_task_events_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<Vec<TaskEvent>> {
    let mut stmt = conn.prepare(
        r"
        SELECT event_id, task_id, event_type, actor, from_status, to_status,
               verification_state, owner_agent_id, execution_action,
               execution_duration_seconds, note, created_at
        FROM task_events
        WHERE task_id = ?1
        ORDER BY rowid
        ",
    )?;
    let rows = stmt.query_map([task_id], map_task_event)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

pub(crate) fn serialize_capabilities(capabilities: &[String]) -> StoreResult<String> {
    serde_json::to_string(capabilities)
        .map_err(|error| StoreError::Validation(format!("invalid capabilities payload: {error}")))
}

pub(crate) fn parse_rfc3339_timestamp(raw: &str) -> StoreResult<OffsetDateTime> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| StoreError::Validation(format!("invalid RFC3339 timestamp: {raw}")))
}

pub(crate) fn parse_database_timestamp(raw: &str) -> StoreResult<OffsetDateTime> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S").map(|dt| dt.and_utc()))
        .map_err(|_| StoreError::Validation(format!("invalid database timestamp: {raw}")))
}

pub(crate) fn is_open_task_status(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Open
            | TaskStatus::Assigned
            | TaskStatus::InProgress
            | TaskStatus::Blocked
            | TaskStatus::ReviewRequired
    )
}

pub(crate) fn validate_handoff_timing(timing: HandoffTiming<'_>) -> StoreResult<()> {
    let due_at = timing.due_at.map(parse_rfc3339_timestamp).transpose()?;
    let expires_at = timing.expires_at.map(parse_rfc3339_timestamp).transpose()?;

    if let (Some(due_at), Some(expires_at)) = (due_at, expires_at)
        && due_at > expires_at
    {
        return Err(StoreError::Validation(
            "handoff due_at must be before expires_at".to_string(),
        ));
    }

    Ok(())
}

pub(crate) fn handoff_is_expired(handoff: &Handoff) -> StoreResult<bool> {
    let Some(expires_at) = handoff.expires_at.as_deref() else {
        return Ok(false);
    };
    Ok(parse_rfc3339_timestamp(expires_at)? <= Utc::now())
}

pub(crate) fn validate_agent_task_link(
    conn: &Connection,
    agent_id: &str,
    status: AgentStatus,
    current_task_id: Option<&str>,
) -> StoreResult<()> {
    match status {
        AgentStatus::Idle if current_task_id.is_some() => {
            return Err(StoreError::Validation(
                "idle heartbeats cannot include a current task".to_string(),
            ));
        }
        AgentStatus::Assigned
        | AgentStatus::InProgress
        | AgentStatus::Blocked
        | AgentStatus::ReviewRequired
            if current_task_id.is_none() =>
        {
            return Err(StoreError::Validation(
                "non-idle heartbeats must include a current task".to_string(),
            ));
        }
        AgentStatus::Idle
        | AgentStatus::Assigned
        | AgentStatus::InProgress
        | AgentStatus::Blocked
        | AgentStatus::ReviewRequired => {}
    }

    let Some(task_id) = current_task_id else {
        return Ok(());
    };

    let task = get_task_in_connection(conn, task_id)?;
    let agent = get_agent_in_connection(conn, agent_id)?;

    if task.project_root != agent.project_root {
        return Err(StoreError::Validation(
            "heartbeat task must belong to the same project as the agent".to_string(),
        ));
    }

    if task.owner_agent_id.as_deref() != Some(agent_id) {
        return Err(StoreError::Validation(
            "heartbeat task must be owned by the reporting agent".to_string(),
        ));
    }

    Ok(())
}

pub(crate) fn validate_agent_registration(
    conn: &Connection,
    agent: &AgentRegistration,
) -> StoreResult<()> {
    match agent.status {
        AgentStatus::Idle if agent.current_task_id.is_some() => {
            return Err(StoreError::Validation(
                "idle registrations cannot include a current task".to_string(),
            ));
        }
        AgentStatus::Assigned
        | AgentStatus::InProgress
        | AgentStatus::Blocked
        | AgentStatus::ReviewRequired
            if agent.current_task_id.is_none() =>
        {
            return Err(StoreError::Validation(
                "non-idle registrations must include a current task".to_string(),
            ));
        }
        AgentStatus::Idle
        | AgentStatus::Assigned
        | AgentStatus::InProgress
        | AgentStatus::Blocked
        | AgentStatus::ReviewRequired => {}
    }

    let Some(task_id) = agent.current_task_id.as_deref() else {
        return Ok(());
    };
    let task = get_task_in_connection(conn, task_id)?;

    if task.project_root != agent.project_root {
        return Err(StoreError::Validation(
            "registration task must belong to the same project as the agent".to_string(),
        ));
    }

    if task.owner_agent_id.as_deref() != Some(agent.agent_id.as_str()) {
        return Err(StoreError::Validation(
            "registration task must be owned by the registering agent".to_string(),
        ));
    }

    Ok(())
}

pub(crate) fn get_task_relationship_in_connection(
    conn: &Connection,
    relationship_id: &str,
) -> StoreResult<TaskRelationship> {
    conn.query_row(
        r"
        SELECT relationship_id, source_task_id, target_task_id, kind, created_by, created_at, updated_at
        FROM task_relationships
        WHERE relationship_id = ?1
        ",
        [relationship_id],
        map_task_relationship,
    )
    .optional()?
    .ok_or(StoreError::NotFound("task relationship"))
}
