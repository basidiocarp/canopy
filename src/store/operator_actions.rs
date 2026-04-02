use crate::models::{
    CouncilMessageType, ExecutionActionKind, HandoffType, OperatorActionKind, Task, TaskEventType,
    TaskRelationshipKind, TaskRelationshipRole, TaskStatus, VerificationState,
};
use rusqlite::params;
use time::OffsetDateTime;

use super::{
    EvidenceLinkRefs, HandoffTiming, Store, StoreError, StoreResult, TaskCreationOptions,
    TaskDeadlineUpdate, TaskEventWrite, TaskOperatorActionInput, TaskStatusUpdate,
    TaskTriageUpdate,
};
use super::helpers::{
    add_council_message_in_connection, add_evidence_in_connection, assign_task_in_connection,
    build_execution_note, compute_open_execution_duration_seconds, create_handoff_in_connection,
    create_task_in_connection, create_task_relationship_in_connection, get_task_in_connection,
    has_active_blockers_in_connection, record_task_event_in_connection,
    release_agent_current_task_in_connection, sync_owner_for_task_status,
    task_has_prior_execution_in_connection, touch_task_in_connection, validate_execution_actor,
};

impl Store {
    #[allow(clippy::too_many_lines)]
    pub(super) fn apply_task_creation_action(
        &self,
        task_id: &str,
        action: OperatorActionKind,
        changed_by: &str,
        input: &TaskOperatorActionInput<'_>,
    ) -> StoreResult<Option<Task>> {
        let current_task = self.get_task(task_id)?;
        if matches!(
            current_task.status,
            TaskStatus::Completed | TaskStatus::Closed | TaskStatus::Cancelled
        ) {
            return Err(StoreError::Validation(format!(
                "operator action {action} is not valid for terminal tasks"
            )));
        }

        let task = match action {
            OperatorActionKind::RecordDecision => self.in_transaction(|conn| {
                let task = get_task_in_connection(conn, task_id)?;
                if task.status != TaskStatus::ReviewRequired
                    || task.verification_state != VerificationState::Pending
                {
                    return Err(StoreError::Validation(
                        "record_decision requires a task awaiting review decision".to_string(),
                    ));
                }

                let task_events = super::helpers::list_task_events_in_connection(conn, task_id)?;
                let review_cycle_context =
                    crate::models::derive_review_cycle_context(&task_events);
                if !review_cycle_context.has_evidence {
                    return Err(StoreError::Validation(
                        "record_decision requires current-cycle evidence support".to_string(),
                    ));
                }
                if review_cycle_context.has_council_decision {
                    return Err(StoreError::Validation(
                        "record_decision requires a task without a current-cycle decision"
                            .to_string(),
                    ));
                }
                if has_active_blockers_in_connection(conn, task_id)?
                    || super::helpers::has_open_follow_up_children_in_connection(conn, task_id)?
                {
                    return Err(StoreError::Validation(
                        "record_decision requires review tasks without graph pressure".to_string(),
                    ));
                }
                if super::helpers::has_unresolved_review_handoffs_in_connection(
                    conn,
                    task_id,
                    &[
                        HandoffType::RequestReview,
                        HandoffType::RequestVerification,
                    ],
                )? {
                    return Err(StoreError::Validation(
                        "record_decision requires review handoff follow-through to resolve first"
                            .to_string(),
                    ));
                }
                if super::helpers::has_unresolved_review_handoffs_in_connection(
                    conn,
                    task_id,
                    &[HandoffType::RecordDecision, HandoffType::CloseTask],
                )? {
                    return Err(StoreError::Validation(
                        "record_decision requires decision handoff follow-through to resolve first"
                            .to_string(),
                    ));
                }

                let message = add_council_message_in_connection(
                    conn,
                    task_id,
                    input.author_agent_id.ok_or_else(|| {
                        StoreError::Validation(
                            "record_decision requires an author_agent_id".to_string(),
                        )
                    })?,
                    CouncilMessageType::Decision,
                    input
                        .message_body
                        .filter(|body| !body.trim().is_empty())
                        .ok_or_else(|| {
                            StoreError::Validation(
                                "record_decision requires a message_body".to_string(),
                            )
                        })?,
                )?;
                let note = format!(
                    "action=record_decision; message_id={}; author_agent_id={}; message_type={}; body={}",
                    message.message_id, message.author_agent_id, message.message_type, message.body
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::CouncilMessagePosted,
                        actor: changed_by,
                        from_status: Some(task.status),
                        to_status: task.status,
                        verification_state: Some(task.verification_state),
                        owner_agent_id: task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(note.as_str()),
                    },
                )?;
                get_task_in_connection(conn, task_id)
            })?,
            OperatorActionKind::CreateHandoff => self.in_transaction(|conn| {
                let handoff = create_handoff_in_connection(
                    conn,
                    task_id,
                    input.from_agent_id.ok_or_else(|| {
                        StoreError::Validation(
                            "create_handoff requires a from_agent_id".to_string(),
                        )
                    })?,
                    input.to_agent_id.ok_or_else(|| {
                        StoreError::Validation("create_handoff requires a to_agent_id".to_string())
                    })?,
                    input.handoff_type.ok_or_else(|| {
                        StoreError::Validation("create_handoff requires a handoff_type".to_string())
                    })?,
                    input
                        .handoff_summary
                        .filter(|summary| !summary.trim().is_empty())
                        .ok_or_else(|| {
                            StoreError::Validation(
                                "create_handoff requires a handoff_summary".to_string(),
                            )
                        })?,
                    input.requested_action,
                    HandoffTiming {
                        due_at: input.due_at,
                        expires_at: input.expires_at,
                    },
                )?;
                let task = get_task_in_connection(conn, task_id)?;
                let note = format!(
                    "handoff_id={}; from_agent_id={}; to_agent_id={}; handoff_type={}; summary={}",
                    handoff.handoff_id,
                    handoff.from_agent_id,
                    handoff.to_agent_id,
                    handoff.handoff_type,
                    handoff.summary
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::HandoffCreated,
                        actor: changed_by,
                        from_status: Some(task.status),
                        to_status: task.status,
                        verification_state: Some(task.verification_state),
                        owner_agent_id: task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(note.as_str()),
                    },
                )?;
                get_task_in_connection(conn, task_id)
            })?,
            OperatorActionKind::PostCouncilMessage => self.in_transaction(|conn| {
                let message = add_council_message_in_connection(
                    conn,
                    task_id,
                    input.author_agent_id.ok_or_else(|| {
                        StoreError::Validation(
                            "post_council_message requires an author_agent_id".to_string(),
                        )
                    })?,
                    input.message_type.ok_or_else(|| {
                        StoreError::Validation(
                            "post_council_message requires a message_type".to_string(),
                        )
                    })?,
                    input
                        .message_body
                        .filter(|body| !body.trim().is_empty())
                        .ok_or_else(|| {
                            StoreError::Validation(
                                "post_council_message requires a message_body".to_string(),
                            )
                        })?,
                )?;
                let task = get_task_in_connection(conn, task_id)?;
                let note = format!(
                    "message_id={}; author_agent_id={}; message_type={}; body={}",
                    message.message_id, message.author_agent_id, message.message_type, message.body
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::CouncilMessagePosted,
                        actor: changed_by,
                        from_status: Some(task.status),
                        to_status: task.status,
                        verification_state: Some(task.verification_state),
                        owner_agent_id: task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(note.as_str()),
                    },
                )?;
                get_task_in_connection(conn, task_id)
            })?,
            OperatorActionKind::AttachEvidence => self.in_transaction(|conn| {
                let evidence = add_evidence_in_connection(
                    conn,
                    task_id,
                    input.evidence_source_kind.ok_or_else(|| {
                        StoreError::Validation(
                            "attach_evidence requires an evidence_source_kind".to_string(),
                        )
                    })?,
                    input
                        .evidence_source_ref
                        .filter(|source_ref| !source_ref.trim().is_empty())
                        .ok_or_else(|| {
                            StoreError::Validation(
                                "attach_evidence requires an evidence_source_ref".to_string(),
                            )
                        })?,
                    input
                        .evidence_label
                        .filter(|label| !label.trim().is_empty())
                        .ok_or_else(|| {
                            StoreError::Validation(
                                "attach_evidence requires an evidence_label".to_string(),
                            )
                        })?,
                    input.evidence_summary,
                    EvidenceLinkRefs {
                        related_handoff_id: input.related_handoff_id,
                        session_id: input.related_session_id,
                        memory_query: input.related_memory_query,
                        symbol: input.related_symbol,
                        file: input.related_file,
                    },
                )?;
                let task = get_task_in_connection(conn, task_id)?;
                let note = format!(
                    "evidence_id={}; source_kind={}; source_ref={}; label={}",
                    evidence.evidence_id, evidence.source_kind, evidence.source_ref, evidence.label
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::EvidenceAttached,
                        actor: changed_by,
                        from_status: Some(task.status),
                        to_status: task.status,
                        verification_state: Some(task.verification_state),
                        owner_agent_id: task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(note.as_str()),
                    },
                )?;
                get_task_in_connection(conn, task_id)
            })?,
            OperatorActionKind::CreateFollowUpTask => self.in_transaction(|conn| {
                let parent_task = get_task_in_connection(conn, task_id)?;
                let follow_up = create_task_in_connection(
                    conn,
                    input
                        .follow_up_title
                        .filter(|title| !title.trim().is_empty())
                        .ok_or_else(|| {
                            StoreError::Validation(
                                "create_follow_up_task requires a follow_up_title".to_string(),
                            )
                        })?,
                    input.follow_up_description,
                    changed_by,
                    &parent_task.project_root,
                    &TaskCreationOptions::default(),
                )?;
                let note = format!(
                    "follow_up_task_id={}; title={}",
                    follow_up.task_id, follow_up.title
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::FollowUpTaskCreated,
                        actor: changed_by,
                        from_status: Some(parent_task.status),
                        to_status: parent_task.status,
                        verification_state: Some(parent_task.verification_state),
                        owner_agent_id: parent_task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(note.as_str()),
                    },
                )?;
                let relationship = create_task_relationship_in_connection(
                    conn,
                    task_id,
                    &follow_up.task_id,
                    TaskRelationshipKind::FollowUp,
                    changed_by,
                )?;
                let relation_note = format!(
                    "relationship_id={}; kind={}; related_task_id={}",
                    relationship.relationship_id, relationship.kind, follow_up.task_id
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::RelationshipUpdated,
                        actor: changed_by,
                        from_status: Some(parent_task.status),
                        to_status: parent_task.status,
                        verification_state: Some(parent_task.verification_state),
                        owner_agent_id: parent_task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(relation_note.as_str()),
                    },
                )?;
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id: &follow_up.task_id,
                        event_type: TaskEventType::RelationshipUpdated,
                        actor: changed_by,
                        from_status: Some(follow_up.status),
                        to_status: follow_up.status,
                        verification_state: Some(follow_up.verification_state),
                        owner_agent_id: follow_up.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(relation_note.as_str()),
                    },
                )?;
                get_task_in_connection(conn, &follow_up.task_id)?;
                touch_task_in_connection(conn, task_id)?;
                get_task_in_connection(conn, task_id)
            })?,
            OperatorActionKind::LinkTaskDependency => self.in_transaction(|conn| {
                let current_task = get_task_in_connection(conn, task_id)?;
                let related_task_id = input.related_task_id.ok_or_else(|| {
                    StoreError::Validation(
                        "link_task_dependency requires a related_task_id".to_string(),
                    )
                })?;
                let relationship_role = input.relationship_role.ok_or_else(|| {
                    StoreError::Validation(
                        "link_task_dependency requires a relationship_role".to_string(),
                    )
                })?;
                let (source_task_id, target_task_id, note_role) = match relationship_role {
                    TaskRelationshipRole::Blocks => {
                        (task_id, related_task_id, TaskRelationshipRole::Blocks)
                    }
                    TaskRelationshipRole::BlockedBy => {
                        (related_task_id, task_id, TaskRelationshipRole::BlockedBy)
                    }
                    TaskRelationshipRole::FollowUpParent
                    | TaskRelationshipRole::FollowUpChild
                    | TaskRelationshipRole::Parent
                    | TaskRelationshipRole::Child => {
                        return Err(StoreError::Validation(
                            "link_task_dependency only supports blocks or blocked_by roles"
                                .to_string(),
                        ));
                    }
                };
                let relationship = create_task_relationship_in_connection(
                    conn,
                    source_task_id,
                    target_task_id,
                    TaskRelationshipKind::Blocks,
                    changed_by,
                )?;
                let related_task = get_task_in_connection(conn, related_task_id)?;
                let note = format!(
                    "relationship_id={}; kind={}; role={}; related_task_id={}; related_title={}",
                    relationship.relationship_id,
                    relationship.kind,
                    note_role,
                    related_task.task_id,
                    related_task.title
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::RelationshipUpdated,
                        actor: changed_by,
                        from_status: Some(current_task.status),
                        to_status: current_task.status,
                        verification_state: Some(current_task.verification_state),
                        owner_agent_id: current_task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(note.as_str()),
                    },
                )?;
                let inverse_role = match relationship_role {
                    TaskRelationshipRole::Blocks => TaskRelationshipRole::BlockedBy,
                    TaskRelationshipRole::BlockedBy => TaskRelationshipRole::Blocks,
                    TaskRelationshipRole::FollowUpParent
                    | TaskRelationshipRole::FollowUpChild
                    | TaskRelationshipRole::Parent
                    | TaskRelationshipRole::Child => {
                        unreachable!("validated above")
                    }
                };
                let inverse_note = format!(
                    "relationship_id={}; kind={}; role={}; related_task_id={}; related_title={}",
                    relationship.relationship_id,
                    relationship.kind,
                    inverse_role,
                    current_task.task_id,
                    current_task.title
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id: &related_task.task_id,
                        event_type: TaskEventType::RelationshipUpdated,
                        actor: changed_by,
                        from_status: Some(related_task.status),
                        to_status: related_task.status,
                        verification_state: Some(related_task.verification_state),
                        owner_agent_id: related_task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(inverse_note.as_str()),
                    },
                )?;
                touch_task_in_connection(conn, related_task_id)?;
                get_task_in_connection(conn, task_id)
            })?,
            _ => return Ok(None),
        };

        Ok(Some(task))
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn apply_task_execution_action(
        &self,
        task_id: &str,
        action: OperatorActionKind,
        changed_by: &str,
        input: &TaskOperatorActionInput<'_>,
    ) -> StoreResult<Option<Task>> {
        let task = match action {
            OperatorActionKind::ClaimTask => self.in_transaction(|conn| {
                let current_task = get_task_in_connection(conn, task_id)?;
                if current_task.owner_agent_id.is_some() || current_task.status != TaskStatus::Open
                {
                    return Err(StoreError::Validation(
                        "claim_task requires an unowned open task".to_string(),
                    ));
                }
                if has_active_blockers_in_connection(conn, task_id)? {
                    return Err(StoreError::Validation(
                        "claim_task requires the task to have no unresolved hard blockers"
                            .to_string(),
                    ));
                }
                let acting_agent_id = input.acting_agent_id.ok_or_else(|| {
                    StoreError::Validation("claim_task requires an acting_agent_id".to_string())
                })?;
                assign_task_in_connection(
                    conn,
                    task_id,
                    acting_agent_id,
                    acting_agent_id,
                    input.note,
                )?;
                let updated = get_task_in_connection(conn, task_id)?;
                let event_note = build_execution_note(changed_by, acting_agent_id, input.note);
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::ExecutionUpdated,
                        actor: acting_agent_id,
                        from_status: Some(current_task.status),
                        to_status: updated.status,
                        verification_state: Some(updated.verification_state),
                        owner_agent_id: updated.owner_agent_id.as_deref(),
                        execution_action: Some(ExecutionActionKind::ClaimTask),
                        execution_duration_seconds: None,
                        note: event_note.as_deref(),
                    },
                )?;
                get_task_in_connection(conn, task_id)
            })?,
            OperatorActionKind::StartTask | OperatorActionKind::ResumeTask => {
                self.in_transaction(|conn| {
                    let action_name = match action {
                        OperatorActionKind::StartTask => "start_task",
                        OperatorActionKind::ResumeTask => "resume_task",
                        _ => unreachable!("execution branch only handles start/resume"),
                    };
                    let execution_action = match action {
                        OperatorActionKind::StartTask => ExecutionActionKind::StartTask,
                        OperatorActionKind::ResumeTask => ExecutionActionKind::ResumeTask,
                        _ => unreachable!("execution branch only handles start/resume"),
                    };
                    let requires_prior_execution = action == OperatorActionKind::ResumeTask;
                    let current_task = get_task_in_connection(conn, task_id)?;
                    if current_task.status != TaskStatus::Assigned {
                        return Err(StoreError::Validation(format!(
                            "{action_name} requires an assigned task"
                        )));
                    }
                    if has_active_blockers_in_connection(conn, task_id)? {
                        return Err(StoreError::Validation(format!(
                            "{action_name} requires the task to have no unresolved hard blockers"
                        )));
                    }
                    let has_prior_execution =
                        task_has_prior_execution_in_connection(conn, task_id)?;
                    if requires_prior_execution && !has_prior_execution {
                        return Err(StoreError::Validation(
                            "resume_task requires a previously started task".to_string(),
                        ));
                    }
                    if !requires_prior_execution && has_prior_execution {
                        return Err(StoreError::Validation(
                            "start_task requires a task that has not started execution yet"
                                .to_string(),
                        ));
                    }
                    let acting_agent_id = validate_execution_actor(
                        &current_task,
                        input.acting_agent_id,
                        action_name,
                    )?;
                    conn.execute(
                        r"
                    UPDATE tasks
                    SET status = 'in_progress',
                        blocked_reason = NULL,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE task_id = ?1
                    ",
                        [task_id],
                    )?;
                    sync_owner_for_task_status(conn, task_id, TaskStatus::InProgress)?;
                    let updated = get_task_in_connection(conn, task_id)?;
                    record_task_event_in_connection(
                        conn,
                        &TaskEventWrite {
                            task_id,
                            event_type: TaskEventType::StatusChanged,
                            actor: acting_agent_id,
                            from_status: Some(current_task.status),
                            to_status: TaskStatus::InProgress,
                            verification_state: Some(updated.verification_state),
                            owner_agent_id: updated.owner_agent_id.as_deref(),
                            execution_action: None,
                            execution_duration_seconds: None,
                            note: None,
                        },
                    )?;
                    let event_note = build_execution_note(changed_by, acting_agent_id, input.note);
                    record_task_event_in_connection(
                        conn,
                        &TaskEventWrite {
                            task_id,
                            event_type: TaskEventType::ExecutionUpdated,
                            actor: acting_agent_id,
                            from_status: Some(current_task.status),
                            to_status: updated.status,
                            verification_state: Some(updated.verification_state),
                            owner_agent_id: updated.owner_agent_id.as_deref(),
                            execution_action: Some(execution_action),
                            execution_duration_seconds: None,
                            note: event_note.as_deref(),
                        },
                    )?;
                    get_task_in_connection(conn, task_id)
                })?
            }
            OperatorActionKind::PauseTask => self.in_transaction(|conn| {
                let current_task = get_task_in_connection(conn, task_id)?;
                if current_task.status != TaskStatus::InProgress {
                    return Err(StoreError::Validation(
                        "pause_task requires an in-progress task".to_string(),
                    ));
                }
                let acting_agent_id =
                    validate_execution_actor(&current_task, input.acting_agent_id, "pause_task")?;
                let duration_seconds = compute_open_execution_duration_seconds(
                    conn,
                    task_id,
                    OffsetDateTime::now_utc(),
                )?;
                conn.execute(
                    r"
                    UPDATE tasks
                    SET status = 'assigned',
                        updated_at = CURRENT_TIMESTAMP
                    WHERE task_id = ?1
                    ",
                    [task_id],
                )?;
                sync_owner_for_task_status(conn, task_id, TaskStatus::Assigned)?;
                let updated = get_task_in_connection(conn, task_id)?;
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::StatusChanged,
                        actor: acting_agent_id,
                        from_status: Some(current_task.status),
                        to_status: TaskStatus::Assigned,
                        verification_state: Some(updated.verification_state),
                        owner_agent_id: updated.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: None,
                    },
                )?;
                let event_note = build_execution_note(changed_by, acting_agent_id, input.note);
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::ExecutionUpdated,
                        actor: acting_agent_id,
                        from_status: Some(current_task.status),
                        to_status: updated.status,
                        verification_state: Some(updated.verification_state),
                        owner_agent_id: updated.owner_agent_id.as_deref(),
                        execution_action: Some(ExecutionActionKind::PauseTask),
                        execution_duration_seconds: duration_seconds,
                        note: event_note.as_deref(),
                    },
                )?;
                get_task_in_connection(conn, task_id)
            })?,
            OperatorActionKind::YieldTask => self.in_transaction(|conn| {
                let current_task = get_task_in_connection(conn, task_id)?;
                if !matches!(
                    current_task.status,
                    TaskStatus::Assigned | TaskStatus::InProgress
                ) {
                    return Err(StoreError::Validation(
                        "yield_task requires an assigned or in-progress task".to_string(),
                    ));
                }
                let acting_agent_id =
                    validate_execution_actor(&current_task, input.acting_agent_id, "yield_task")?;
                let duration_seconds = if current_task.status == TaskStatus::InProgress {
                    compute_open_execution_duration_seconds(
                        conn,
                        task_id,
                        OffsetDateTime::now_utc(),
                    )?
                } else {
                    None
                };
                conn.execute(
                    r"
                    UPDATE tasks
                    SET owner_agent_id = NULL,
                        status = 'open',
                        blocked_reason = NULL,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE task_id = ?1
                    ",
                    [task_id],
                )?;
                release_agent_current_task_in_connection(conn, acting_agent_id, task_id)?;
                let updated = get_task_in_connection(conn, task_id)?;
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::StatusChanged,
                        actor: acting_agent_id,
                        from_status: Some(current_task.status),
                        to_status: TaskStatus::Open,
                        verification_state: Some(updated.verification_state),
                        owner_agent_id: updated.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: None,
                    },
                )?;
                let event_note = build_execution_note(changed_by, acting_agent_id, input.note);
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::ExecutionUpdated,
                        actor: acting_agent_id,
                        from_status: Some(current_task.status),
                        to_status: updated.status,
                        verification_state: Some(updated.verification_state),
                        owner_agent_id: None,
                        execution_action: Some(ExecutionActionKind::YieldTask),
                        execution_duration_seconds: duration_seconds,
                        note: event_note.as_deref(),
                    },
                )?;
                get_task_in_connection(conn, task_id)
            })?,
            OperatorActionKind::CompleteTask => self.in_transaction(|conn| {
                let current_task = get_task_in_connection(conn, task_id)?;
                if current_task.status == TaskStatus::Blocked {
                    return Err(StoreError::Validation(
                        "complete_task cannot complete a blocked task".to_string(),
                    ));
                }
                if !matches!(
                    current_task.status,
                    TaskStatus::Assigned | TaskStatus::InProgress
                ) {
                    return Err(StoreError::Validation(
                        "complete_task requires an assigned or in-progress task".to_string(),
                    ));
                }
                let acting_agent_id = validate_execution_actor(
                    &current_task,
                    input.acting_agent_id,
                    "complete_task",
                )?;
                let duration_seconds = if current_task.status == TaskStatus::InProgress {
                    compute_open_execution_duration_seconds(
                        conn,
                        task_id,
                        OffsetDateTime::now_utc(),
                    )?
                } else {
                    None
                };
                conn.execute(
                    r"
                    UPDATE tasks
                    SET status = 'review_required',
                        verification_state = ?2,
                        blocked_reason = NULL,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE task_id = ?1
                    ",
                    params![task_id, VerificationState::Pending.to_string()],
                )?;
                sync_owner_for_task_status(conn, task_id, TaskStatus::ReviewRequired)?;
                let updated = get_task_in_connection(conn, task_id)?;
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::StatusChanged,
                        actor: acting_agent_id,
                        from_status: Some(current_task.status),
                        to_status: TaskStatus::ReviewRequired,
                        verification_state: Some(updated.verification_state),
                        owner_agent_id: updated.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: None,
                    },
                )?;
                let event_note = build_execution_note(changed_by, acting_agent_id, input.note);
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::ExecutionUpdated,
                        actor: acting_agent_id,
                        from_status: Some(current_task.status),
                        to_status: updated.status,
                        verification_state: Some(updated.verification_state),
                        owner_agent_id: updated.owner_agent_id.as_deref(),
                        execution_action: Some(ExecutionActionKind::CompleteTask),
                        execution_duration_seconds: duration_seconds,
                        note: event_note.as_deref(),
                    },
                )?;
                get_task_in_connection(conn, task_id)
            })?,
            _ => return Ok(None),
        };

        Ok(Some(task))
    }
}

// --- Free functions for operator action dispatch ---

pub(super) fn task_operator_triage_update<'a>(
    action: OperatorActionKind,
    input: &'a TaskOperatorActionInput<'a>,
) -> StoreResult<Option<TaskTriageUpdate<'a>>> {
    let update = match action {
        OperatorActionKind::AcknowledgeTask => TaskTriageUpdate {
            acknowledged: Some(true),
            event_note: input.note,
            ..TaskTriageUpdate::default()
        },
        OperatorActionKind::UnacknowledgeTask => TaskTriageUpdate {
            acknowledged: Some(false),
            event_note: input.note,
            ..TaskTriageUpdate::default()
        },
        OperatorActionKind::SetTaskPriority => TaskTriageUpdate {
            priority: Some(input.priority.ok_or_else(|| {
                StoreError::Validation("set_task_priority requires a priority value".to_string())
            })?),
            event_note: input.note,
            ..TaskTriageUpdate::default()
        },
        OperatorActionKind::SetTaskSeverity => TaskTriageUpdate {
            severity: Some(input.severity.ok_or_else(|| {
                StoreError::Validation("set_task_severity requires a severity value".to_string())
            })?),
            event_note: input.note,
            ..TaskTriageUpdate::default()
        },
        OperatorActionKind::UpdateTaskNote => TaskTriageUpdate {
            owner_note: input.owner_note,
            clear_owner_note: input.clear_owner_note,
            event_note: input.note,
            ..TaskTriageUpdate::default()
        },
        _ => return Ok(None),
    };

    Ok(Some(update))
}

pub(super) fn task_operator_deadline_update<'a>(
    action: OperatorActionKind,
    input: &'a TaskOperatorActionInput<'a>,
) -> StoreResult<Option<TaskDeadlineUpdate<'a>>> {
    let update = match action {
        OperatorActionKind::SetTaskDueAt => TaskDeadlineUpdate {
            due_at: Some(input.due_at.ok_or_else(|| {
                StoreError::Validation("set_task_due_at requires a due_at value".to_string())
            })?),
            event_note: input.note,
            ..TaskDeadlineUpdate::default()
        },
        OperatorActionKind::ClearTaskDueAt => TaskDeadlineUpdate {
            clear_due_at: true,
            event_note: input.note,
            ..TaskDeadlineUpdate::default()
        },
        OperatorActionKind::SetReviewDueAt => TaskDeadlineUpdate {
            review_due_at: Some(input.review_due_at.ok_or_else(|| {
                StoreError::Validation(
                    "set_review_due_at requires a review_due_at value".to_string(),
                )
            })?),
            event_note: input.note,
            ..TaskDeadlineUpdate::default()
        },
        OperatorActionKind::ClearReviewDueAt => TaskDeadlineUpdate {
            clear_review_due_at: true,
            event_note: input.note,
            ..TaskDeadlineUpdate::default()
        },
        _ => return Ok(None),
    };

    Ok(Some(update))
}

#[allow(clippy::too_many_lines)]
pub(super) fn task_operator_status_update<'a>(
    task: &Task,
    action: OperatorActionKind,
    input: &'a TaskOperatorActionInput<'a>,
) -> StoreResult<Option<(TaskStatus, TaskStatusUpdate<'a>)>> {
    let update = match action {
        OperatorActionKind::VerifyTask => {
            if !(task.status == TaskStatus::ReviewRequired
                || matches!(
                    task.verification_state,
                    VerificationState::Pending | VerificationState::Failed
                ))
            {
                return Err(StoreError::Validation(
                    "verify_task requires a task that is awaiting or repeating review".to_string(),
                ));
            }
            let verification_state = input.verification_state.ok_or_else(|| {
                StoreError::Validation(
                    "verify_task requires a verification_state value".to_string(),
                )
            })?;
            if verification_state == VerificationState::Unknown {
                return Err(StoreError::Validation(
                    "verify_task requires a concrete verification_state".to_string(),
                ));
            }
            if verification_state == VerificationState::Passed {
                return Err(StoreError::Validation(
                    "verify_task no longer accepts passed; use close_task".to_string(),
                ));
            }

            (
                TaskStatus::ReviewRequired,
                TaskStatusUpdate {
                    verification_state: Some(verification_state),
                    event_note: input.note,
                    ..TaskStatusUpdate::default()
                },
            )
        }
        OperatorActionKind::CloseTask => {
            if task.status != TaskStatus::ReviewRequired
                || task.verification_state != VerificationState::Pending
            {
                return Err(StoreError::Validation(
                    "close_task requires a task awaiting review closeout".to_string(),
                ));
            }
            if input
                .closure_summary
                .is_none_or(|summary| summary.trim().is_empty())
            {
                return Err(StoreError::Validation(
                    "close_task requires a closure summary".to_string(),
                ));
            }

            (
                TaskStatus::Completed,
                TaskStatusUpdate {
                    verification_state: Some(VerificationState::Passed),
                    closure_summary: input.closure_summary,
                    event_note: input.note,
                    ..TaskStatusUpdate::default()
                },
            )
        }
        OperatorActionKind::BlockTask => (
            TaskStatus::Blocked,
            TaskStatusUpdate {
                blocked_reason: Some(input.blocked_reason.ok_or_else(|| {
                    StoreError::Validation("block_task requires a blocked reason".to_string())
                })?),
                event_note: input.note,
                ..TaskStatusUpdate::default()
            },
        ),
        OperatorActionKind::UnblockTask => {
            if task.status != TaskStatus::Blocked {
                return Err(StoreError::Validation(
                    "only blocked tasks can be unblocked".to_string(),
                ));
            }
            let target_status = if task.owner_agent_id.is_some() {
                TaskStatus::Assigned
            } else {
                TaskStatus::Open
            };
            (
                target_status,
                TaskStatusUpdate {
                    event_note: input.note,
                    ..TaskStatusUpdate::default()
                },
            )
        }
        OperatorActionKind::ReopenBlockedTaskWhenUnblocked => {
            if task.status != TaskStatus::Blocked {
                return Err(StoreError::Validation(
                    "reopen_blocked_task_when_unblocked requires a blocked task".to_string(),
                ));
            }
            (
                if task.owner_agent_id.is_some() {
                    TaskStatus::Assigned
                } else {
                    TaskStatus::Open
                },
                TaskStatusUpdate {
                    event_note: input.note,
                    ..TaskStatusUpdate::default()
                },
            )
        }
        _ => return Ok(None),
    };

    Ok(Some(update))
}
