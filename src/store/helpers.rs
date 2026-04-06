use crate::models::{
    AgentHeartbeatEvent, AgentHeartbeatSource, AgentRegistration, AgentRole, AgentStatus,
    EvidenceRef, EvidenceSourceKind, ExecutionActionKind, FileLock, Handoff, HandoffStatus,
    HandoffType, Task, TaskAssignment, TaskEvent, TaskEventType, TaskPriority, TaskRelationship,
    TaskRelationshipKind, TaskStatus, VerificationState, capabilities_match, parse_capabilities,
};
use rusqlite::{Connection, OptionalExtension, params, types::Type};
use std::str::FromStr;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use ulid::Ulid;

use super::{
    AgentHeartbeatWrite, EVIDENCE_REF_SCHEMA_VERSION, EvidenceLinkRefs, HandoffTiming, StoreError,
    StoreResult, TaskCreationOptions, TaskEventWrite,
};

// --- Row Mappers ---

pub(crate) fn map_agent(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRegistration> {
    Ok(AgentRegistration {
        agent_id: row.get(0)?,
        host_id: row.get(1)?,
        host_type: row.get(2)?,
        host_instance: row.get(3)?,
        model: row.get(4)?,
        project_root: row.get(5)?,
        worktree_id: row.get(6)?,
        role: parse_optional_enum_column(row, 7)?,
        capabilities: row
            .get::<_, Option<String>>(8)?
            .map_or_else(Vec::new, |json| parse_capabilities(&json)),
        status: parse_enum_column(row, 9)?,
        current_task_id: row.get(10)?,
        heartbeat_at: row.get(11)?,
    })
}

pub(crate) fn map_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        task_id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        requested_by: row.get(3)?,
        project_root: row.get(4)?,
        parent_task_id: row.get(25)?,
        required_role: parse_optional_enum_column(row, 5)?,
        required_capabilities: row
            .get::<_, Option<String>>(6)?
            .map_or_else(Vec::new, |json| parse_capabilities(&json)),
        auto_review: row.get::<_, Option<i64>>(7)?.unwrap_or(0) != 0,
        verification_required: row.get::<_, Option<i64>>(8)?.unwrap_or(0) != 0,
        status: parse_enum_column(row, 9)?,
        verification_state: parse_enum_column(row, 10)?,
        priority: parse_enum_column(row, 11)?,
        severity: parse_enum_column(row, 12)?,
        owner_agent_id: row.get(13)?,
        owner_note: row.get(14)?,
        acknowledged_by: row.get(15)?,
        acknowledged_at: row.get(16)?,
        blocked_reason: row.get(17)?,
        verified_by: row.get(18)?,
        verified_at: row.get(19)?,
        closed_by: row.get(20)?,
        closure_summary: row.get(21)?,
        closed_at: row.get(22)?,
        due_at: row.get(23)?,
        review_due_at: row.get(24)?,
        scope: row
            .get::<_, Option<String>>(26)?
            .map_or_else(Vec::new, |json| parse_capabilities(&json)),
        created_at: row.get(27)?,
        updated_at: row.get(28)?,
    })
}

pub(crate) fn map_handoff(row: &rusqlite::Row<'_>) -> rusqlite::Result<Handoff> {
    Ok(Handoff {
        handoff_id: row.get(0)?,
        task_id: row.get(1)?,
        from_agent_id: row.get(2)?,
        to_agent_id: row.get(3)?,
        handoff_type: parse_enum_column(row, 4)?,
        summary: row.get(5)?,
        requested_action: row.get(6)?,
        due_at: row.get(7)?,
        expires_at: row.get(8)?,
        status: parse_enum_column(row, 9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        resolved_at: row.get(12)?,
    })
}

pub(crate) fn map_task_assignment(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskAssignment> {
    Ok(TaskAssignment {
        assignment_id: row.get(0)?,
        task_id: row.get(1)?,
        assigned_to: row.get(2)?,
        assigned_by: row.get(3)?,
        reason: row.get(4)?,
        assigned_at: row.get(5)?,
    })
}

pub(crate) fn map_council_message(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<crate::models::CouncilMessage> {
    Ok(crate::models::CouncilMessage {
        message_id: row.get(0)?,
        task_id: row.get(1)?,
        author_agent_id: row.get(2)?,
        message_type: parse_enum_column(row, 3)?,
        body: row.get(4)?,
    })
}

pub(crate) fn map_evidence(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvidenceRef> {
    let schema_version: String = row.get(11)?;
    if schema_version != EVIDENCE_REF_SCHEMA_VERSION {
        return Err(rusqlite::Error::FromSqlConversionFailure(
            11,
            Type::Text,
            format!(
                "unsupported evidence schema_version: {schema_version} (expected {EVIDENCE_REF_SCHEMA_VERSION})"
            )
            .into(),
        ));
    }

    Ok(EvidenceRef {
        schema_version,
        evidence_id: row.get(0)?,
        task_id: row.get(1)?,
        source_kind: parse_enum_column(row, 2)?,
        source_ref: row.get(3)?,
        label: row.get(4)?,
        summary: row.get(5)?,
        related_handoff_id: row.get(6)?,
        related_session_id: row.get(7)?,
        related_memory_query: row.get(8)?,
        related_symbol: row.get(9)?,
        related_file: row.get(10)?,
    })
}

pub(crate) fn map_task_relationship(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRelationship> {
    Ok(TaskRelationship {
        relationship_id: row.get(0)?,
        source_task_id: row.get(1)?,
        target_task_id: row.get(2)?,
        kind: parse_enum_column(row, 3)?,
        created_by: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub(crate) fn map_task_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskEvent> {
    Ok(TaskEvent {
        event_id: row.get(0)?,
        task_id: row.get(1)?,
        event_type: parse_enum_column(row, 2)?,
        actor: row.get(3)?,
        from_status: parse_optional_enum_column(row, 4)?,
        to_status: parse_enum_column(row, 5)?,
        verification_state: parse_optional_enum_column(row, 6)?,
        owner_agent_id: row.get(7)?,
        execution_action: parse_optional_enum_column(row, 8)?,
        execution_duration_seconds: row.get(9)?,
        note: row.get(10)?,
        created_at: row.get(11)?,
    })
}

pub(crate) fn map_agent_heartbeat(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentHeartbeatEvent> {
    Ok(AgentHeartbeatEvent {
        heartbeat_id: row.get(0)?,
        agent_id: row.get(1)?,
        status: parse_enum_column(row, 2)?,
        current_task_id: row.get(3)?,
        related_task_id: row.get(4)?,
        source: parse_enum_column(row, 5)?,
        created_at: row.get(6)?,
    })
}

pub(crate) fn map_file_lock(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileLock> {
    Ok(FileLock {
        lock_id: row.get(0)?,
        task_id: row.get(1)?,
        agent_id: row.get(2)?,
        file_path: row.get(3)?,
        worktree_id: row.get(4)?,
        locked_at: row.get(5)?,
        released_at: row.get(6)?,
    })
}

pub(crate) fn parse_enum_column<T>(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    <T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
{
    let value: String = row.get(index)?;
    parse_enum_value::<T>(&value, index)
}

pub(crate) fn parse_optional_enum_column<T>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<T>>
where
    T: FromStr,
    <T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
{
    let value: Option<String> = row.get(index)?;
    value
        .map(|value| parse_enum_value::<T>(&value, index))
        .transpose()
}

pub(crate) fn parse_enum_value<T>(value: &str, index: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    <T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
{
    T::from_str(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
    })
}

// --- Connection-level helpers used across multiple domain modules ---

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
    OffsetDateTime::parse(raw, &Rfc3339)
        .map_err(|_| StoreError::Validation(format!("invalid RFC3339 timestamp: {raw}")))
}

pub(crate) fn parse_database_timestamp(raw: &str) -> StoreResult<OffsetDateTime> {
    OffsetDateTime::parse(raw, &Rfc3339)
        .or_else(|_| {
            time::PrimitiveDateTime::parse(
                raw,
                &time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
            )
            .map(time::PrimitiveDateTime::assume_utc)
        })
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
    Ok(parse_rfc3339_timestamp(expires_at)? <= OffsetDateTime::now_utc())
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

pub(crate) fn create_task_in_connection(
    conn: &Connection,
    title: &str,
    description: Option<&str>,
    requested_by: &str,
    project_root: &str,
    options: &TaskCreationOptions,
) -> StoreResult<Task> {
    let task = Task {
        task_id: Ulid::new().to_string(),
        title: title.to_string(),
        description: description.map(ToOwned::to_owned),
        requested_by: requested_by.to_string(),
        project_root: project_root.to_string(),
        required_role: options.required_role,
        required_capabilities: options.required_capabilities.clone(),
        auto_review: options.auto_review,
        verification_required: options.verification_required,
        status: TaskStatus::Open,
        verification_state: VerificationState::Unknown,
        priority: TaskPriority::Medium,
        severity: crate::models::TaskSeverity::None,
        owner_agent_id: None,
        owner_note: None,
        acknowledged_by: None,
        acknowledged_at: None,
        blocked_reason: None,
        verified_by: None,
        verified_at: None,
        closed_by: None,
        closure_summary: None,
        closed_at: None,
        due_at: None,
        review_due_at: None,
        parent_task_id: None,
        scope: options.scope.clone(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    conn.execute(
        r"
        INSERT INTO tasks (
            task_id, title, description, requested_by, project_root, required_role, required_capabilities, auto_review, verification_required, status,
            verification_state, priority, severity, owner_agent_id, owner_note,
            acknowledged_by, acknowledged_at, blocked_reason, verified_by, verified_at,
            closed_by, closure_summary, closed_at, due_at, review_due_at, parent_task_id, scope, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ",
        params![
            task.task_id,
            task.title,
            task.description,
            task.requested_by,
            task.project_root,
            task.required_role.map(|value| value.to_string()),
            serialize_capabilities(&task.required_capabilities)?,
            i64::from(task.auto_review),
            i64::from(task.verification_required),
            task.status.to_string(),
            task.verification_state.to_string(),
            task.priority.to_string(),
            task.severity.to_string(),
            task.owner_agent_id,
            task.owner_note,
            task.acknowledged_by,
            task.acknowledged_at,
            task.blocked_reason,
            task.verified_by,
            task.verified_at,
            task.closed_by,
            task.closure_summary,
            task.closed_at,
            task.due_at,
            task.review_due_at,
            task.parent_task_id,
            serialize_capabilities(&task.scope)?,
        ],
    )?;
    record_task_event_in_connection(
        conn,
        &TaskEventWrite {
            task_id: &task.task_id,
            event_type: TaskEventType::Created,
            actor: requested_by,
            from_status: None,
            to_status: TaskStatus::Open,
            verification_state: Some(VerificationState::Unknown),
            owner_agent_id: None,
            execution_action: None,
            execution_duration_seconds: None,
            note: description,
        },
    )?;
    get_task_in_connection(conn, &task.task_id)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn create_handoff_in_connection(
    conn: &Connection,
    task_id: &str,
    from_agent_id: &str,
    to_agent_id: &str,
    handoff_type: HandoffType,
    summary: &str,
    requested_action: Option<&str>,
    timing: HandoffTiming<'_>,
) -> StoreResult<Handoff> {
    get_task_in_connection(conn, task_id)?;
    get_agent_in_connection(conn, from_agent_id)?;
    get_agent_in_connection(conn, to_agent_id)?;
    if from_agent_id == to_agent_id {
        return Err(StoreError::Validation(
            "handoff source and target agents must differ".to_string(),
        ));
    }
    validate_handoff_timing(timing)?;

    let handoff = Handoff {
        handoff_id: Ulid::new().to_string(),
        task_id: task_id.to_string(),
        from_agent_id: from_agent_id.to_string(),
        to_agent_id: to_agent_id.to_string(),
        handoff_type,
        summary: summary.to_string(),
        requested_action: requested_action.map(ToOwned::to_owned),
        due_at: timing.due_at.map(ToOwned::to_owned),
        expires_at: timing.expires_at.map(ToOwned::to_owned),
        status: HandoffStatus::Open,
        created_at: String::new(),
        updated_at: String::new(),
        resolved_at: None,
    };
    conn.execute(
        r"
        INSERT INTO handoffs (
            handoff_id, task_id, from_agent_id, to_agent_id, handoff_type,
            summary, requested_action, due_at, expires_at, status, created_at, updated_at, resolved_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL)
        ",
        params![
            handoff.handoff_id,
            handoff.task_id,
            handoff.from_agent_id,
            handoff.to_agent_id,
            handoff.handoff_type.to_string(),
            handoff.summary,
            handoff.requested_action,
            handoff.due_at,
            handoff.expires_at,
            handoff.status.to_string(),
        ],
    )?;
    touch_task_in_connection(conn, task_id)?;
    get_handoff_in_connection(conn, &handoff.handoff_id)
}

pub(crate) fn add_council_message_in_connection(
    conn: &Connection,
    task_id: &str,
    author_agent_id: &str,
    message_type: crate::models::CouncilMessageType,
    body: &str,
) -> StoreResult<crate::models::CouncilMessage> {
    get_task_in_connection(conn, task_id)?;
    get_agent_in_connection(conn, author_agent_id)?;
    if body.trim().is_empty() {
        return Err(StoreError::Validation(
            "council messages require a non-empty body".to_string(),
        ));
    }

    let message = crate::models::CouncilMessage {
        message_id: Ulid::new().to_string(),
        task_id: task_id.to_string(),
        author_agent_id: author_agent_id.to_string(),
        message_type,
        body: body.to_string(),
    };
    conn.execute(
        r"
        INSERT INTO council_messages (message_id, task_id, author_agent_id, message_type, body)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ",
        params![
            message.message_id,
            message.task_id,
            message.author_agent_id,
            message.message_type.to_string(),
            message.body
        ],
    )?;
    touch_task_in_connection(conn, task_id)?;
    Ok(message)
}

pub(crate) fn add_evidence_in_connection(
    conn: &Connection,
    task_id: &str,
    source_kind: EvidenceSourceKind,
    source_ref: &str,
    label: &str,
    summary: Option<&str>,
    links: EvidenceLinkRefs<'_>,
) -> StoreResult<EvidenceRef> {
    get_task_in_connection(conn, task_id)?;
    if source_ref.trim().is_empty() || label.trim().is_empty() {
        return Err(StoreError::Validation(
            "evidence requires a non-empty source_ref and label".to_string(),
        ));
    }
    if let Some(handoff_id) = links.related_handoff_id {
        let handoff = get_handoff_in_connection(conn, handoff_id)?;
        if handoff.task_id != task_id {
            return Err(StoreError::Validation(
                "related handoff must belong to the same task".to_string(),
            ));
        }
    }

    let navigation = normalize_evidence_navigation(
        source_kind,
        source_ref,
        links.session_id,
        links.memory_query,
        links.symbol,
        links.file,
    );

    let evidence = EvidenceRef {
        schema_version: EVIDENCE_REF_SCHEMA_VERSION.to_string(),
        evidence_id: Ulid::new().to_string(),
        task_id: task_id.to_string(),
        source_kind,
        source_ref: source_ref.to_string(),
        label: label.to_string(),
        summary: summary.map(ToOwned::to_owned),
        related_handoff_id: links.related_handoff_id.map(ToOwned::to_owned),
        related_session_id: navigation.session_id.map(ToOwned::to_owned),
        related_memory_query: navigation.memory_query.map(ToOwned::to_owned),
        related_symbol: navigation.symbol.map(ToOwned::to_owned),
        related_file: navigation.file.map(ToOwned::to_owned),
    };
    conn.execute(
        r"
        INSERT INTO evidence_refs (
            schema_version, evidence_id, task_id, source_kind, source_ref, label, summary, related_handoff_id,
            related_session_id, related_memory_query, related_symbol, related_file
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
        params![
            evidence.schema_version,
            evidence.evidence_id,
            evidence.task_id,
            evidence.source_kind.to_string(),
            evidence.source_ref,
            evidence.label,
            evidence.summary,
            evidence.related_handoff_id,
            evidence.related_session_id,
            evidence.related_memory_query,
            evidence.related_symbol,
            evidence.related_file,
        ],
    )?;
    touch_task_in_connection(conn, task_id)?;
    Ok(evidence)
}

pub(crate) fn create_task_relationship_in_connection(
    conn: &Connection,
    source_task_id: &str,
    target_task_id: &str,
    kind: TaskRelationshipKind,
    created_by: &str,
) -> StoreResult<TaskRelationship> {
    let source_task = get_task_in_connection(conn, source_task_id)?;
    let target_task = get_task_in_connection(conn, target_task_id)?;
    if source_task_id == target_task_id {
        return Err(StoreError::Validation(
            "task relationships must link two different tasks".to_string(),
        ));
    }
    if source_task.project_root != target_task.project_root {
        return Err(StoreError::Validation(
            "task relationships must stay within the same project".to_string(),
        ));
    }
    if kind == TaskRelationshipKind::Parent {
        let existing_parent = conn
            .query_row(
                r"
                SELECT parent_task_id
                FROM tasks
                WHERE task_id = ?1
                LIMIT 1
                ",
                [source_task_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        if existing_parent.is_some() {
            return Err(StoreError::Validation(
                "task already has a parent".to_string(),
            ));
        }
        if parent_chain_contains_task_in_connection(conn, target_task_id, source_task_id)? {
            return Err(StoreError::Validation(
                "parent relationship would create a cycle".to_string(),
            ));
        }
    }
    let duplicate = conn
        .query_row(
            r"
            SELECT relationship_id
            FROM task_relationships
            WHERE source_task_id = ?1 AND target_task_id = ?2 AND kind = ?3
            ",
            params![source_task_id, target_task_id, kind.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if duplicate.is_some() {
        return Err(StoreError::Validation(
            "task relationship already exists".to_string(),
        ));
    }

    let relationship = TaskRelationship {
        relationship_id: Ulid::new().to_string(),
        source_task_id: source_task_id.to_string(),
        target_task_id: target_task_id.to_string(),
        kind,
        created_by: created_by.to_string(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    conn.execute(
        r"
        INSERT INTO task_relationships (
            relationship_id, source_task_id, target_task_id, kind, created_by, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ",
        params![
            relationship.relationship_id,
            relationship.source_task_id,
            relationship.target_task_id,
            relationship.kind.to_string(),
            relationship.created_by,
        ],
    )?;
    if kind == TaskRelationshipKind::Parent {
        conn.execute(
            r"
            UPDATE tasks
            SET parent_task_id = ?2,
                updated_at = CURRENT_TIMESTAMP
            WHERE task_id = ?1
            ",
            params![source_task_id, target_task_id],
        )?;
    }
    touch_task_in_connection(conn, source_task_id)?;
    touch_task_in_connection(conn, target_task_id)?;
    get_task_relationship_in_connection(conn, &relationship.relationship_id)
}

pub(crate) fn record_parent_relationship_in_connection(
    conn: &Connection,
    child_task_id: &str,
    parent_task_id: &str,
    created_by: &str,
) -> StoreResult<TaskRelationship> {
    let child_task = get_task_in_connection(conn, child_task_id)?;
    let parent_task = get_task_in_connection(conn, parent_task_id)?;
    let relationship = create_task_relationship_in_connection(
        conn,
        child_task_id,
        parent_task_id,
        TaskRelationshipKind::Parent,
        created_by,
    )?;
    let child_note = format!(
        "relationship_id={}; kind={}; role={}; related_task_id={}; related_title={}",
        relationship.relationship_id,
        relationship.kind,
        crate::models::TaskRelationshipRole::Parent,
        parent_task.task_id,
        parent_task.title
    );
    record_task_event_in_connection(
        conn,
        &TaskEventWrite {
            task_id: child_task_id,
            event_type: TaskEventType::RelationshipUpdated,
            actor: created_by,
            from_status: Some(child_task.status),
            to_status: child_task.status,
            verification_state: Some(child_task.verification_state),
            owner_agent_id: child_task.owner_agent_id.as_deref(),
            execution_action: None,
            execution_duration_seconds: None,
            note: Some(child_note.as_str()),
        },
    )?;
    let parent_note = format!(
        "relationship_id={}; kind={}; role={}; related_task_id={}; related_title={}",
        relationship.relationship_id,
        relationship.kind,
        crate::models::TaskRelationshipRole::Child,
        child_task.task_id,
        child_task.title
    );
    record_task_event_in_connection(
        conn,
        &TaskEventWrite {
            task_id: parent_task_id,
            event_type: TaskEventType::RelationshipUpdated,
            actor: created_by,
            from_status: Some(parent_task.status),
            to_status: parent_task.status,
            verification_state: Some(parent_task.verification_state),
            owner_agent_id: parent_task.owner_agent_id.as_deref(),
            execution_action: None,
            execution_duration_seconds: None,
            note: Some(parent_note.as_str()),
        },
    )?;
    Ok(relationship)
}

pub(crate) fn find_task_relationship_between_in_connection(
    conn: &Connection,
    task_id: &str,
    related_task_id: &str,
    kind: TaskRelationshipKind,
) -> StoreResult<Option<TaskRelationship>> {
    conn.query_row(
        r"
        SELECT relationship_id, source_task_id, target_task_id, kind, created_by, created_at, updated_at
        FROM task_relationships
        WHERE kind = ?1
          AND (
            (source_task_id = ?2 AND target_task_id = ?3)
            OR (source_task_id = ?3 AND target_task_id = ?2)
          )
        ORDER BY created_at DESC
        LIMIT 1
        ",
        params![kind.to_string(), task_id, related_task_id],
        map_task_relationship,
    )
    .optional()
    .map_err(StoreError::from)
}

pub(crate) fn list_source_task_relationships_in_connection(
    conn: &Connection,
    task_id: &str,
    kind: TaskRelationshipKind,
) -> StoreResult<Vec<TaskRelationship>> {
    let mut statement = conn.prepare(
        r"
        SELECT relationship_id, source_task_id, target_task_id, kind, created_by, created_at, updated_at
        FROM task_relationships
        WHERE source_task_id = ?1 AND kind = ?2
        ORDER BY created_at ASC
        ",
    )?;
    let rows = statement.query_map(params![task_id, kind.to_string()], map_task_relationship)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

pub(crate) fn parent_chain_contains_task_in_connection(
    conn: &Connection,
    start_task_id: &str,
    candidate_ancestor_task_id: &str,
) -> StoreResult<bool> {
    let mut stmt = conn.prepare(
        r"
        WITH RECURSIVE ancestry(task_id) AS (
            SELECT target_task_id
            FROM task_relationships
            WHERE source_task_id = ?1
              AND kind = 'parent'
            UNION
            SELECT rel.target_task_id
            FROM task_relationships rel
            INNER JOIN ancestry ON rel.source_task_id = ancestry.task_id
            WHERE rel.kind = 'parent'
        )
        SELECT 1
        FROM ancestry
        WHERE task_id = ?2
        LIMIT 1
        ",
    )?;
    stmt.exists(params![start_task_id, candidate_ancestor_task_id])
        .map_err(StoreError::from)
}

pub(crate) fn delete_task_relationship_in_connection(
    conn: &Connection,
    relationship_id: &str,
) -> StoreResult<()> {
    let relationship = get_task_relationship_in_connection(conn, relationship_id)?;
    let deleted = conn.execute(
        "DELETE FROM task_relationships WHERE relationship_id = ?1",
        [relationship_id],
    )?;
    if deleted == 0 {
        return Err(StoreError::NotFound("task relationship"));
    }
    if let Some(child_task_id) = match relationship.kind {
        TaskRelationshipKind::Parent => Some(relationship.source_task_id.as_str()),
        TaskRelationshipKind::FollowUp | TaskRelationshipKind::Blocks => None,
    } {
        conn.execute(
            r"
            UPDATE tasks
            SET parent_task_id = NULL,
                updated_at = CURRENT_TIMESTAMP
            WHERE task_id = ?1
            ",
            [child_task_id],
        )?;
    }
    touch_task_in_connection(conn, &relationship.source_task_id)?;
    touch_task_in_connection(conn, &relationship.target_task_id)?;
    Ok(())
}

pub(crate) fn has_open_child_tasks_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<bool> {
    let mut stmt = conn.prepare(
        r"
        SELECT tasks.status
        FROM tasks
        WHERE tasks.parent_task_id = ?1
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
    let mut current_task_id = Some(task_id.to_string());
    while let Some(candidate_task_id) = current_task_id {
        maybe_auto_complete_task_in_connection(conn, &candidate_task_id, changed_by)?;
        current_task_id = conn
            .query_row(
                r"
                SELECT parent_task_id
                FROM tasks
                WHERE task_id = ?1
                ORDER BY created_at DESC
                LIMIT 1
                ",
                [candidate_task_id.as_str()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
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
        if status != TaskStatus::Completed {
            return Ok(());
        }
    }
    if !has_children {
        return Ok(());
    }

    if !task.verification_required {
        return Ok(());
    }

    if task.verification_state != VerificationState::Passed
        || !has_passing_script_verification_in_connection(conn, task_id)?
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
               summary, requested_action, due_at, expires_at, status, created_at, updated_at, resolved_at
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
    let elapsed = (now - started_at).whole_seconds();
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
        || task_priority_rank(task.priority) < task_priority_rank(super::AUTO_REVIEW_MIN_PRIORITY)
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
    let mut review_task_ids = Vec::with_capacity(super::AUTO_REVIEW_SUBTASKS.len());

    for (title, instruction) in super::AUTO_REVIEW_SUBTASKS {
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
    .map(|value| value.flatten())
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
