#![allow(clippy::wildcard_imports)]

use super::*;

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
