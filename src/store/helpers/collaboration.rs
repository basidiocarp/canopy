#![allow(clippy::wildcard_imports)]

use super::*;

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
        created_at: None,
    };
    conn.execute(
        r"
        INSERT INTO council_messages (message_id, task_id, author_agent_id, message_type, body, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
        ",
        params![
            message.message_id,
            message.task_id,
            message.author_agent_id,
            message.message_type.to_string(),
            message.body
        ],
    )?;
    conn.execute(
        r"
        UPDATE council_sessions
        SET updated_at = CURRENT_TIMESTAMP
        WHERE task_id = ?1
        ",
        [task_id],
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
