use chrono::Utc;
use rusqlite::{ErrorCode, OptionalExtension, params};
use std::collections::HashMap;

use super::helpers::{
    assign_task_in_connection, create_task_in_connection, get_task_in_connection,
    has_passing_script_verification_in_connection, is_open_task_status,
    list_open_children_in_connection, map_task, maybe_auto_complete_task_tree_in_connection,
    record_parent_relationship_in_connection, record_task_event_in_connection,
    sync_owner_for_task_status, sync_task_workflow_in_connection,
};
use super::operator_actions::{
    task_operator_deadline_update, task_operator_status_update, task_operator_triage_update,
};
use super::{
    Store, StoreError, StoreResult, TaskCreationOptions, TaskDeadlineUpdate, TaskEventWrite,
    TaskOperatorActionInput, TaskStatusUpdate, TaskTriageUpdate,
};
use crate::models::{
    AgentRole, HandoffStatus, HandoffType, Notification, NotificationEventType, OperatorActionKind, Task, TaskAction, TaskEventType,
    TaskRelationship, TaskRelationshipRole, TaskStatus, TaskSummary, VerificationState,
    capabilities_match, derive_review_cycle_context,
};

use super::helpers::{handoff_is_expired, parse_enum_value};

impl Store {
    /// Creates a new task in the local ledger.
    ///
    /// # Errors
    ///
    /// Returns an error if the task cannot be persisted.
    pub fn create_task(
        &self,
        title: &str,
        description: Option<&str>,
        requested_by: &str,
        project_root: &str,
        required_role: Option<AgentRole>,
    ) -> StoreResult<Task> {
        self.create_task_with_options(
            title,
            description,
            requested_by,
            project_root,
            &TaskCreationOptions {
                required_role,
                ..TaskCreationOptions::default()
            },
        )
    }

    /// Creates a new task in the local ledger with explicit option fields.
    ///
    /// # Errors
    ///
    /// Returns an error if the task cannot be persisted.
    pub fn create_task_with_options(
        &self,
        title: &str,
        description: Option<&str>,
        requested_by: &str,
        project_root: &str,
        options: &TaskCreationOptions,
    ) -> StoreResult<Task> {
        self.in_transaction(|conn| {
            create_task_in_connection(
                conn,
                title,
                description,
                requested_by,
                project_root,
                options,
            )
        })
    }

    /// Enqueues a new scoped task, preventing duplicates at the database level.
    ///
    /// When `scope` is non-empty, a partial unique index ensures only one task
    /// with that scope can be in `open` (queued) status at a time. A second
    /// enqueue attempt for the same scope returns
    /// [`StoreError::DuplicateQueuedTask`] instead of a raw database error.
    ///
    /// Tasks with an empty scope (`options.scope` is empty) are created without
    /// uniqueness enforcement — multiple unscoped tasks can coexist.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::DuplicateQueuedTask`] when a queued task for the
    /// same scope already exists. Returns other [`StoreError`] variants on
    /// database or validation failures.
    pub fn enqueue_task(
        &self,
        title: &str,
        description: Option<&str>,
        requested_by: &str,
        project_root: &str,
        options: &TaskCreationOptions,
    ) -> StoreResult<Task> {
        let scope_display = if options.scope.is_empty() {
            String::new()
        } else {
            options.scope.join(", ")
        };
        self.create_task_with_options(title, description, requested_by, project_root, options)
            .map_err(|err| {
                if is_unique_constraint_error(&err) && !scope_display.is_empty() {
                    StoreError::DuplicateQueuedTask {
                        scope: scope_display.clone(),
                    }
                } else {
                    err
                }
            })
    }

    /// Creates a new task and links it as a child of an existing parent task.
    ///
    /// # Errors
    ///
    /// Returns an error if the parent task does not exist, the child task
    /// cannot be created, or the parent relationship is invalid.
    pub fn create_subtask(
        &self,
        parent_task_id: &str,
        title: &str,
        description: Option<&str>,
        requested_by: &str,
        required_role: Option<AgentRole>,
    ) -> StoreResult<Task> {
        self.create_subtask_with_options(
            parent_task_id,
            title,
            description,
            requested_by,
            &TaskCreationOptions {
                required_role,
                ..TaskCreationOptions::default()
            },
        )
    }

    /// Creates a new task and links it as a child of an existing parent task
    /// with explicit option fields.
    ///
    /// # Errors
    ///
    /// Returns an error if the parent task does not exist, the child task
    /// cannot be created, or the parent relationship is invalid.
    pub fn create_subtask_with_options(
        &self,
        parent_task_id: &str,
        title: &str,
        description: Option<&str>,
        requested_by: &str,
        options: &TaskCreationOptions,
    ) -> StoreResult<Task> {
        self.in_transaction(|conn| {
            // Parent existence check is inside the transaction to avoid a TOCTOU
            // race between an external check and the moment the parent is fetched.
            let parent_task = get_task_in_connection(conn, parent_task_id)?;
            let child_task = create_task_in_connection(
                conn,
                title,
                description,
                requested_by,
                &parent_task.project_root,
                options,
            )?;
            record_parent_relationship_in_connection(
                conn,
                &child_task.task_id,
                parent_task_id,
                requested_by,
            )?;
            get_task_in_connection(conn, &child_task.task_id)
        })
    }

    /// Links an existing task under a parent task in the same project.
    ///
    /// # Errors
    ///
    /// Returns an error if either task does not exist or the parent link is invalid.
    pub fn link_parent_task(
        &self,
        child_task_id: &str,
        parent_task_id: &str,
        created_by: &str,
    ) -> StoreResult<TaskRelationship> {
        self.ensure_task_exists(child_task_id)?;
        self.ensure_task_exists(parent_task_id)?;
        self.in_transaction(|conn| {
            record_parent_relationship_in_connection(
                conn,
                child_task_id,
                parent_task_id,
                created_by,
            )
        })
    }

    /// Assigns a task to an agent and records the assignment event.
    ///
    /// # Errors
    ///
    /// Returns an error if the task or agent does not exist or if the database
    /// update fails.
    pub fn assign_task(
        &self,
        task_id: &str,
        assigned_to: &str,
        assigned_by: &str,
        reason: Option<&str>,
    ) -> StoreResult<Task> {
        self.ensure_agent_exists(assigned_to)?;
        self.ensure_task_exists(task_id)?;
        self.in_transaction(|conn| {
            assign_task_in_connection(conn, task_id, assigned_to, assigned_by, reason)?;
            get_task_in_connection(conn, task_id)
        })
    }

    /// Lists tasks in creation order.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_tasks(&self) -> StoreResult<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT task_id, title, description, requested_by, project_root, parent_task_id,
                   queue_state_id, worktree_binding_id, execution_session_ref, review_cycle_id,
                   workflow_id, phase_id,
                   required_role, required_capabilities, auto_review, verification_required, status, verification_state, priority, severity, owner_agent_id, owner_note,
                   acknowledged_by, acknowledged_at, blocked_reason, verified_by,
                   verified_at, closed_by, closure_summary, closed_at, due_at, review_due_at,
                   scope, created_at, updated_at
            FROM tasks
            ORDER BY rowid
            ",
        )?;
        let rows = stmt.query_map([], map_task)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Loads a single task by id.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn get_task(&self, task_id: &str) -> StoreResult<Task> {
        get_task_in_connection(&self.conn, task_id)
    }

    /// Updates task lifecycle, verification, and closure metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist, the requested transition is
    /// invalid, or the update fails.
    #[allow(clippy::too_many_lines)]
    pub fn update_task_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        changed_by: &str,
        update: TaskStatusUpdate<'_>,
    ) -> StoreResult<Task> {
        let _span = tracing::info_span!("canopy.task.update_status").entered();
        self.ensure_task_exists(task_id)?;
        self.in_transaction(|conn| {
            let current = get_task_in_connection(conn, task_id)?;
            let from_status = current.status;
            let next_verification = update
                .verification_state
                .unwrap_or(current.verification_state);

            if from_status != status && !from_status.allowed_transitions().contains(&status) {
                return Err(StoreError::Validation(format!(
                    "cannot transition from {from_status} to {status}"
                )));
            }

            if status == TaskStatus::Blocked && update.blocked_reason.is_none() {
                return Err(StoreError::Validation(
                    "blocked tasks require a blocked reason".to_string(),
                ));
            }

            if status == TaskStatus::Completed {
                let open_children = list_open_children_in_connection(conn, task_id)?;
                if !open_children.is_empty() {
                    let blocking = open_children
                        .iter()
                        .map(|(id, title, st)| format!("{id} ({title}, status={st})"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(StoreError::Validation(format!(
                        "tasks cannot complete while child tasks remain open: {blocking}"
                    )));
                }
                if current.verification_required {
                    if next_verification != VerificationState::Passed {
                        return Err(StoreError::Validation(format!(
                            "task {task_id} requires passing verification. Run: canopy task verify --task-id {task_id} --script <path>"
                        )));
                    }
                    if !has_passing_script_verification_in_connection(conn, task_id)? {
                        return Err(StoreError::Validation(format!(
                            "task {task_id} requires script verification evidence. Run: canopy task verify --task-id {task_id} --script <path>"
                        )));
                    }
                }
            }

            let (verified_by, verified_at) = if update.verification_state.is_some() {
                (Some(changed_by), Some("CURRENT_TIMESTAMP"))
            } else {
                (current.verified_by.as_deref(), None)
            };

            let is_terminal = matches!(
                status,
                TaskStatus::Completed | TaskStatus::Closed | TaskStatus::Cancelled
            );

            conn.execute(
                r"
                UPDATE tasks
                SET status = ?2,
                    verification_state = ?3,
                    blocked_reason = ?4,
                    verified_by = ?5,
                    verified_at = COALESCE(?6, verified_at),
                    closed_by = ?7,
                    closure_summary = ?8,
                    closed_at = CASE WHEN ?9 THEN CURRENT_TIMESTAMP ELSE NULL END,
                    updated_at = CURRENT_TIMESTAMP
                WHERE task_id = ?1
                ",
                params![
                    task_id,
                    status.to_string(),
                    next_verification.to_string(),
                    if status == TaskStatus::Blocked {
                        update.blocked_reason
                    } else {
                        None
                    },
                    verified_by,
                    verified_at,
                    if is_terminal { Some(changed_by) } else { None },
                    if is_terminal {
                        update.closure_summary
                    } else {
                        None
                    },
                    is_terminal,
                ],
            )?;

            sync_owner_for_task_status(conn, task_id, status)?;
            sync_task_workflow_in_connection(conn, task_id)?;

            // Emit notification for status transitions
            if matches!(
                status,
                TaskStatus::Completed | TaskStatus::Blocked | TaskStatus::Cancelled
            ) {
                let event_type = match status {
                    TaskStatus::Completed => NotificationEventType::TaskCompleted,
                    TaskStatus::Blocked => NotificationEventType::TaskBlocked,
                    TaskStatus::Cancelled => NotificationEventType::TaskCancelled,
                    _ => unreachable!(),
                };
                let notif = Notification {
                    notification_id: ulid::Ulid::new().to_string(),
                    event_type,
                    task_id: Some(task_id.to_string()),
                    agent_id: None,
                    payload: serde_json::json!({}),
                    seen: false,
                    created_at: chrono::Utc::now().to_rfc3339(),
                };
                // Ignore notification emission errors — notification failure must not fail the task update
                let _ = super::notifications::insert_notification(conn, &notif);
            }

            let updated = get_task_in_connection(conn, task_id)?;
            let mut notes = Vec::new();
            if let Some(note) = match status {
                TaskStatus::Blocked => update.blocked_reason,
                TaskStatus::Completed | TaskStatus::Closed | TaskStatus::Cancelled => {
                    update.closure_summary
                }
                _ => None,
            } {
                notes.push(note.to_string());
            }
            if let Some(event_note) = update.event_note {
                notes.push(format!("note={event_note}"));
            }
            let note = (!notes.is_empty()).then(|| notes.join("; "));
            record_task_event_in_connection(
                conn,
                &TaskEventWrite {
                    task_id,
                    event_type: TaskEventType::StatusChanged,
                    actor: changed_by,
                    from_status: Some(from_status),
                    to_status: status,
                    verification_state: Some(updated.verification_state),
                    owner_agent_id: updated.owner_agent_id.as_deref(),
                    execution_action: None,
                    execution_duration_seconds: None,
                    note: note.as_deref(),
                },
            )?;
            maybe_auto_complete_task_tree_in_connection(conn, task_id, changed_by)?;
            get_task_in_connection(conn, task_id)
        })
    }

    /// Updates operator triage metadata without changing the task lifecycle.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist, no triage fields were
    /// provided, or the update fails.
    #[allow(clippy::too_many_lines)]
    pub fn update_task_triage(
        &self,
        task_id: &str,
        changed_by: &str,
        update: TaskTriageUpdate<'_>,
    ) -> StoreResult<Task> {
        self.ensure_task_exists(task_id)?;
        self.in_transaction(|conn| {
            let current = get_task_in_connection(conn, task_id)?;
            let next_priority = update.priority.unwrap_or(current.priority);
            let next_severity = update.severity.unwrap_or(current.severity);
            let next_owner_note = if update.clear_owner_note {
                None
            } else {
                update
                    .owner_note
                    .map(ToOwned::to_owned)
                    .or_else(|| current.owner_note.clone())
            };
            let next_acknowledged_by = match update.acknowledged {
                Some(true) => Some(changed_by.to_string()),
                Some(false) => None,
                None => current.acknowledged_by.clone(),
            };
            let preserve_acknowledged_at = update.acknowledged.is_none();

            if update.priority.is_none()
                && update.severity.is_none()
                && update.acknowledged.is_none()
                && update.owner_note.is_none()
                && !update.clear_owner_note
            {
                return Err(StoreError::Validation(
                    "triage update requires at least one field".to_string(),
                ));
            }

            conn.execute(
                r"
                UPDATE tasks
                SET priority = ?2,
                    severity = ?3,
                    owner_note = ?4,
                    acknowledged_by = ?5,
                    acknowledged_at = CASE
                        WHEN ?6 THEN acknowledged_at
                        WHEN ?7 THEN CURRENT_TIMESTAMP
                        ELSE NULL
                    END,
                    updated_at = CURRENT_TIMESTAMP
                WHERE task_id = ?1
                ",
                params![
                    task_id,
                    next_priority.to_string(),
                    next_severity.to_string(),
                    next_owner_note,
                    next_acknowledged_by,
                    preserve_acknowledged_at,
                    update.acknowledged.unwrap_or(false),
                ],
            )?;

            let updated = get_task_in_connection(conn, task_id)?;
            let mut notes = Vec::new();
            if let Some(priority) = update.priority {
                notes.push(format!("priority:{}->{}", current.priority, priority));
            }
            if let Some(severity) = update.severity {
                notes.push(format!("severity:{}->{}", current.severity, severity));
            }
            if let Some(acknowledged) = update.acknowledged {
                notes.push(format!(
                    "acknowledged:{}->{}",
                    current.acknowledged_at.is_some(),
                    acknowledged
                ));
            }
            if update.owner_note.is_some() || update.clear_owner_note {
                let next_owner_note = updated.owner_note.as_deref().unwrap_or("");
                let previous_owner_note = current.owner_note.as_deref().unwrap_or("");
                notes.push(format!(
                    "owner_note:{previous_owner_note:?}->{next_owner_note:?}"
                ));
            }
            if let Some(event_note) = update.event_note {
                notes.push(format!("note={event_note}"));
            }
            let note = if notes.is_empty() {
                None
            } else {
                Some(notes.join("; "))
            };

            record_task_event_in_connection(
                conn,
                &TaskEventWrite {
                    task_id,
                    event_type: TaskEventType::TriageUpdated,
                    actor: changed_by,
                    from_status: Some(updated.status),
                    to_status: updated.status,
                    verification_state: Some(updated.verification_state),
                    owner_agent_id: updated.owner_agent_id.as_deref(),
                    execution_action: None,
                    execution_duration_seconds: None,
                    note: note.as_deref(),
                },
            )?;
            Ok(updated)
        })
    }

    /// Updates task deadline metadata without changing ownership or lifecycle state.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist, no deadline fields were
    /// provided, or a supplied deadline is invalid for the current task state.
    pub fn update_task_deadlines(
        &self,
        task_id: &str,
        changed_by: &str,
        update: TaskDeadlineUpdate<'_>,
    ) -> StoreResult<Task> {
        self.ensure_task_exists(task_id)?;
        self.in_transaction(|conn| {
            let current = get_task_in_connection(conn, task_id)?;

            if update.due_at.is_none()
                && update.review_due_at.is_none()
                && !update.clear_due_at
                && !update.clear_review_due_at
            {
                return Err(StoreError::Validation(
                    "deadline update requires at least one field".to_string(),
                ));
            }

            if update.due_at.is_some()
                && (!is_open_task_status(current.status)
                    || current.status == TaskStatus::ReviewRequired)
            {
                return Err(StoreError::Validation(
                    "set_task_due_at requires a non-terminal task outside review".to_string(),
                ));
            }
            if update.review_due_at.is_some() && current.status != TaskStatus::ReviewRequired {
                return Err(StoreError::Validation(
                    "set_review_due_at requires a task in review".to_string(),
                ));
            }

            if let Some(due_at) = update.due_at {
                super::helpers::parse_rfc3339_timestamp(due_at)?;
            }
            if let Some(review_due_at) = update.review_due_at {
                super::helpers::parse_rfc3339_timestamp(review_due_at)?;
            }

            let next_due_at = if update.clear_due_at {
                None
            } else {
                update
                    .due_at
                    .map(ToOwned::to_owned)
                    .or_else(|| current.due_at.clone())
            };
            let next_review_due_at = if update.clear_review_due_at {
                None
            } else {
                update
                    .review_due_at
                    .map(ToOwned::to_owned)
                    .or_else(|| current.review_due_at.clone())
            };

            conn.execute(
                r"
                UPDATE tasks
                SET due_at = ?2,
                    review_due_at = ?3,
                    updated_at = CURRENT_TIMESTAMP
                WHERE task_id = ?1
                ",
                params![task_id, next_due_at, next_review_due_at],
            )?;

            let updated = get_task_in_connection(conn, task_id)?;
            let mut notes = Vec::new();
            if update.due_at.is_some() || update.clear_due_at {
                notes.push(format!(
                    "due_at:{:?}->{:?}",
                    current.due_at.as_deref(),
                    updated.due_at.as_deref()
                ));
            }
            if update.review_due_at.is_some() || update.clear_review_due_at {
                notes.push(format!(
                    "review_due_at:{:?}->{:?}",
                    current.review_due_at.as_deref(),
                    updated.review_due_at.as_deref()
                ));
            }
            if let Some(event_note) = update.event_note {
                notes.push(format!("note={event_note}"));
            }
            let note = (!notes.is_empty()).then(|| notes.join("; "));
            record_task_event_in_connection(
                conn,
                &TaskEventWrite {
                    task_id,
                    event_type: TaskEventType::DeadlineUpdated,
                    actor: changed_by,
                    from_status: Some(current.status),
                    to_status: updated.status,
                    verification_state: Some(updated.verification_state),
                    owner_agent_id: updated.owner_agent_id.as_deref(),
                    execution_action: None,
                    execution_duration_seconds: None,
                    note: note.as_deref(),
                },
            )?;
            Ok(updated)
        })
    }

    /// Applies a task-scoped operator action using runtime-owned semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if the action is invalid for tasks, required fields are
    /// missing, or the underlying write fails.
    #[allow(clippy::too_many_lines)]
    pub fn apply_task_operator_action(
        &self,
        task_id: &str,
        changed_by: &str,
        task_action: TaskAction<'_>,
    ) -> StoreResult<Task> {
        let action = task_action.action_kind();
        let input = TaskOperatorActionInput::from(&task_action);
        if let Some(update) = task_operator_triage_update(action, &input)? {
            return self.update_task_triage(task_id, changed_by, update);
        }

        if let Some(update) = task_operator_deadline_update(action, &input)? {
            return self.update_task_deadlines(task_id, changed_by, update);
        }

        if let Some(task) = self.apply_task_execution_action(task_id, action, changed_by, &input)? {
            return Ok(task);
        }

        let current_task = self.get_task(task_id)?;
        if let Some((status, update)) = task_operator_status_update(&current_task, action, &input)?
        {
            if action == OperatorActionKind::CloseTask {
                let review_cycle_context =
                    derive_review_cycle_context(&self.list_task_events(task_id)?);
                if !review_cycle_context.has_evidence {
                    return Err(StoreError::Validation(
                        "close_task requires current-cycle evidence support".to_string(),
                    ));
                }
                if !review_cycle_context.has_council_decision {
                    return Err(StoreError::Validation(
                        "close_task requires a current-cycle decision context".to_string(),
                    ));
                }
                if self
                    .list_related_tasks(task_id)?
                    .into_iter()
                    .any(|related| {
                        (related.relationship_role == TaskRelationshipRole::BlockedBy
                            && matches!(
                                related.status,
                                TaskStatus::Open
                                    | TaskStatus::Assigned
                                    | TaskStatus::InProgress
                                    | TaskStatus::Blocked
                                    | TaskStatus::ReviewRequired
                            ))
                            || (related.relationship_role == TaskRelationshipRole::FollowUpChild
                                && matches!(
                                    related.status,
                                    TaskStatus::Open
                                        | TaskStatus::Assigned
                                        | TaskStatus::InProgress
                                        | TaskStatus::Blocked
                                        | TaskStatus::ReviewRequired
                                ))
                            || (related.relationship_role == TaskRelationshipRole::Child
                                && matches!(
                                    related.status,
                                    TaskStatus::Open
                                        | TaskStatus::Assigned
                                        | TaskStatus::InProgress
                                        | TaskStatus::Blocked
                                        | TaskStatus::ReviewRequired
                                ))
                    })
                {
                    return Err(StoreError::Validation(
                        "close_task requires review tasks without unresolved graph pressure"
                            .to_string(),
                    ));
                }
                if self
                    .list_handoffs(Some(task_id))?
                    .into_iter()
                    .any(|handoff| {
                        matches!(
                            handoff.handoff_type,
                            HandoffType::RequestReview
                                | HandoffType::RequestVerification
                                | HandoffType::RecordDecision
                                | HandoffType::CloseTask
                        ) && match handoff.status {
                            HandoffStatus::Open => !handoff_is_expired(&handoff).unwrap_or(false),
                            HandoffStatus::Accepted => true,
                            HandoffStatus::Rejected
                            | HandoffStatus::Expired
                            | HandoffStatus::Cancelled
                            | HandoffStatus::Completed => false,
                        }
                    })
                {
                    return Err(StoreError::Validation(
                        "close_task requires review handoff follow-through to resolve first"
                            .to_string(),
                    ));
                }
            }
            if action == OperatorActionKind::ReopenBlockedTaskWhenUnblocked
                && self
                    .list_related_tasks(task_id)?
                    .into_iter()
                    .any(|related| related.relationship_role == TaskRelationshipRole::BlockedBy)
            {
                return Err(StoreError::Validation(
                    "reopen_blocked_task_when_unblocked requires the task to have no remaining blockers"
                        .to_string(),
                ));
            }
            return self.update_task_status(task_id, status, changed_by, update);
        }

        if let Some(task) = self.apply_task_creation_action(task_id, action, changed_by, &input)? {
            return Ok(task);
        }

        if let Some(task) = self.apply_task_graph_action(task_id, action, changed_by, &input)? {
            return Ok(task);
        }

        match action {
            OperatorActionKind::ReassignTask => self.assign_task(
                task_id,
                input.assigned_to.ok_or_else(|| {
                    StoreError::Validation(
                        "reassign_task requires an assigned_to agent".to_string(),
                    )
                })?,
                changed_by,
                input.note,
            ),
            OperatorActionKind::AcceptHandoff
            | OperatorActionKind::RejectHandoff
            | OperatorActionKind::CancelHandoff
            | OperatorActionKind::CompleteHandoff
            | OperatorActionKind::FollowUpHandoff
            | OperatorActionKind::ExpireHandoff => Err(StoreError::Validation(format!(
                "operator action {action} is not valid for tasks"
            ))),
            OperatorActionKind::AcknowledgeTask
            | OperatorActionKind::UnacknowledgeTask
            | OperatorActionKind::VerifyTask
            | OperatorActionKind::RecordDecision
            | OperatorActionKind::CloseTask
            | OperatorActionKind::ClaimTask
            | OperatorActionKind::StartTask
            | OperatorActionKind::ResumeTask
            | OperatorActionKind::PauseTask
            | OperatorActionKind::YieldTask
            | OperatorActionKind::CompleteTask
            | OperatorActionKind::ResolveDependency
            | OperatorActionKind::ReopenBlockedTaskWhenUnblocked
            | OperatorActionKind::PromoteFollowUp
            | OperatorActionKind::CloseFollowUpChain
            | OperatorActionKind::SetTaskPriority
            | OperatorActionKind::SetTaskSeverity
            | OperatorActionKind::BlockTask
            | OperatorActionKind::UnblockTask
            | OperatorActionKind::UpdateTaskNote
            | OperatorActionKind::SetTaskDueAt
            | OperatorActionKind::ClearTaskDueAt
            | OperatorActionKind::SetReviewDueAt
            | OperatorActionKind::ClearReviewDueAt
            | OperatorActionKind::CreateHandoff
            | OperatorActionKind::SummonCouncilSession
            | OperatorActionKind::PostCouncilMessage
            | OperatorActionKind::AttachEvidence
            | OperatorActionKind::CreateFollowUpTask
            | OperatorActionKind::LinkTaskDependency => unreachable!("handled above"),
        }
    }

    /// Lists tasks filtered by project root and/or status.
    ///
    /// Pass `None` for any parameter to skip that filter. When `status` is
    /// non-empty, only tasks with one of those statuses are returned.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_tasks_filtered(
        &self,
        project_root: Option<&str>,
        status: Option<&[TaskStatus]>,
        limit: Option<i64>,
    ) -> StoreResult<Vec<Task>> {
        let select = r"
            SELECT task_id, title, description, requested_by, project_root, parent_task_id,
                   queue_state_id, worktree_binding_id, execution_session_ref, review_cycle_id,
                   workflow_id, phase_id,
                   required_role, required_capabilities, auto_review, verification_required, status, verification_state, priority, severity, owner_agent_id, owner_note,
                   acknowledged_by, acknowledged_at, blocked_reason, verified_by,
                   verified_at, closed_by, closure_summary, closed_at, due_at, review_due_at,
                   scope, created_at, updated_at
            FROM tasks
        ";

        let mut conditions: Vec<String> = Vec::new();
        if project_root.is_some() {
            conditions.push("project_root = ?1".to_string());
        }

        let status_placeholder_start = if project_root.is_some() {
            2usize
        } else {
            1usize
        };
        let status_count = status.map_or(0, <[TaskStatus]>::len);
        if status_count > 0 {
            let placeholders: Vec<String> = (status_placeholder_start
                ..status_placeholder_start + status_count)
                .map(|i| format!("?{i}"))
                .collect();
            conditions.push(format!("status IN ({})", placeholders.join(", ")));
        }

        let limit_placeholder = status_placeholder_start + status_count;
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let limit_clause = if limit.is_some() {
            format!("LIMIT ?{limit_placeholder}")
        } else {
            String::new()
        };
        let sql = format!("{select} {where_clause} ORDER BY rowid {limit_clause}");

        let mut stmt = self.conn.prepare(&sql)?;

        let mut param_idx = 1usize;
        if let Some(pr) = project_root {
            stmt.raw_bind_parameter(param_idx, pr)?;
            param_idx += 1;
        }
        if let Some(statuses) = status {
            for s in statuses {
                stmt.raw_bind_parameter(param_idx, s.to_string())?;
                param_idx += 1;
            }
        }
        if let Some(lim) = limit {
            // param_idx now equals limit_placeholder; assert this to catch any
            // future filter additions that shift the offset without updating the
            // placeholder arithmetic above.
            debug_assert_eq!(
                param_idx, limit_placeholder,
                "limit_placeholder offset mismatch: expected {limit_placeholder}, got {param_idx}"
            );
            stmt.raw_bind_parameter(param_idx, lim)?;
        }

        let mut rows = stmt.raw_query();
        let mut tasks = Vec::new();
        while let Some(row) = rows.next()? {
            tasks.push(map_task(row)?);
        }
        Ok(tasks)
    }

    /// Counts tasks grouped by status, optionally scoped to a project.
    ///
    /// Returns a map of `status_string -> count`.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn count_tasks_by_status(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<HashMap<String, i64>> {
        let mut counts = HashMap::new();
        if let Some(project_root) = project_root {
            let mut stmt = self.conn.prepare(
                r"
                SELECT status, COUNT(*) as cnt
                FROM tasks
                WHERE project_root = ?1
                GROUP BY status
                ",
            )?;
            let mut rows = stmt.query([project_root])?;
            while let Some(row) = rows.next()? {
                let status: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                counts.insert(status, count);
            }
        } else {
            let mut stmt = self.conn.prepare(
                r"
                SELECT status, COUNT(*) as cnt
                FROM tasks
                GROUP BY status
                ",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let status: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                counts.insert(status, count);
            }
        }
        Ok(counts)
    }

    /// Clear the owner assignment on a task so it becomes available for
    /// claiming again. Used when yielding a task back to the open pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub fn clear_task_assignment(&self, task_id: &str) -> StoreResult<()> {
        self.in_transaction(|conn| {
            conn.execute(
                "UPDATE tasks SET owner_agent_id = NULL, updated_at = CURRENT_TIMESTAMP WHERE task_id = ?1",
                params![task_id],
            )?;
            sync_task_workflow_in_connection(conn, task_id)?;
            Ok(())
        })
    }

    /// Atomically claim a task. Returns the task if successful, None if already claimed.
    /// Uses UPDATE...WHERE to prevent TOCTOU races between concurrent agents.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub fn atomic_claim_task(&self, agent_id: &str, task_id: &str) -> StoreResult<Option<Task>> {
        self.in_transaction(|conn| {
            let now = Utc::now().to_rfc3339();
            let rows_affected = conn.execute(
                r"
                UPDATE tasks
                SET status = 'assigned',
                    owner_agent_id = ?1,
                    updated_at = ?2
                WHERE task_id = ?3
                  AND status = 'open'
                  AND owner_agent_id IS NULL
                ",
                params![agent_id, now, task_id],
            )?;
            if rows_affected > 0 {
                // Record the claim event so the audit trail has no gaps.
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::StatusChanged,
                        actor: agent_id,
                        from_status: Some(TaskStatus::Open),
                        to_status: TaskStatus::Assigned,
                        verification_state: None,
                        owner_agent_id: Some(agent_id),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some("claimed via atomic_claim_task"),
                    },
                )?;
                sync_task_workflow_in_connection(conn, task_id)?;
                let task = get_task_in_connection(conn, task_id)?;
                Ok(Some(task))
            } else {
                Ok(None)
            }
        })
    }

    /// Atomically claim a task while enforcing a per-agent concurrency cap.
    ///
    /// If the agent already has `concurrency_cap` or more active (non-terminal)
    /// tasks, the claim is refused with [`StoreError::ConcurrencyCapReached`]
    /// instead of panicking or silently over-assigning.
    ///
    /// The cap and the claim transition happen inside a single `BEGIN IMMEDIATE`
    /// transaction, so two racing callers cannot both bypass the cap.
    ///
    /// Returns the newly claimed task when successful, or `None` when the task
    /// is already owned by another agent.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::ConcurrencyCapReached`] when the agent is at or
    /// over its cap. Returns other [`StoreError`] variants on database failures.
    pub fn atomic_claim_task_with_cap(
        &self,
        agent_id: &str,
        task_id: &str,
        concurrency_cap: i64,
    ) -> StoreResult<Option<Task>> {
        self.in_transaction(|conn| {
            // Count active (non-terminal) tasks already claimed by this agent.
            let claimed: i64 = conn.query_row(
                r"
                SELECT COUNT(*)
                FROM tasks
                WHERE owner_agent_id = ?1
                  AND status NOT IN ('completed', 'closed', 'cancelled')
                ",
                params![agent_id],
                |row| row.get(0),
            )?;

            if claimed >= concurrency_cap {
                return Err(StoreError::ConcurrencyCapReached {
                    agent_id: agent_id.to_string(),
                    claimed,
                    cap: concurrency_cap,
                });
            }

            let now = Utc::now().to_rfc3339();
            let rows_affected = conn.execute(
                r"
                UPDATE tasks
                SET status = 'assigned',
                    owner_agent_id = ?1,
                    updated_at = ?2
                WHERE task_id = ?3
                  AND status = 'open'
                  AND owner_agent_id IS NULL
                ",
                params![agent_id, now, task_id],
            )?;

            if rows_affected > 0 {
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::StatusChanged,
                        actor: agent_id,
                        from_status: Some(TaskStatus::Open),
                        to_status: TaskStatus::Assigned,
                        verification_state: None,
                        owner_agent_id: Some(agent_id),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some("claimed via atomic_claim_task_with_cap"),
                    },
                )?;
                sync_task_workflow_in_connection(conn, task_id)?;
                let task = get_task_in_connection(conn, task_id)?;
                Ok(Some(task))
            } else {
                Ok(None)
            }
        })
    }

    /// Query tasks available for claiming, filtered by role/capabilities.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn query_available_tasks(
        &self,
        role: Option<&str>,
        capabilities: &[String],
        project_root: Option<&str>,
        limit: i64,
    ) -> StoreResult<Vec<Task>> {
        let mut sql = String::from(
            r"
            SELECT task_id, title, description, requested_by, project_root, parent_task_id,
                   queue_state_id, worktree_binding_id, execution_session_ref, review_cycle_id,
                   workflow_id, phase_id,
                   required_role, required_capabilities, auto_review, verification_required, status,
                   verification_state, priority, severity, owner_agent_id, owner_note,
                   acknowledged_by, acknowledged_at, blocked_reason, verified_by,
                   verified_at, closed_by, closure_summary, closed_at, due_at, review_due_at,
                   scope, created_at, updated_at
            FROM tasks
            WHERE status = 'open' AND owner_agent_id IS NULL
            ",
        );
        let priority_order = " ORDER BY CASE priority WHEN 'critical' THEN 4 WHEN 'high' THEN 3 WHEN 'medium' THEN 2 WHEN 'low' THEN 1 ELSE 0 END DESC, created_at ASC";
        if project_root.is_some() {
            sql.push_str(" AND project_root = ?1");
            sql.push_str(priority_order);
            sql.push_str(" LIMIT ?2");
        } else {
            sql.push_str(priority_order);
            sql.push_str(" LIMIT ?1");
        }

        let mut stmt = self.conn.prepare(&sql)?;
        let tasks: Vec<Task> = if let Some(root) = project_root {
            let rows = stmt.query_map(params![root, limit], map_task)?;
            rows.collect::<Result<Vec<_>, _>>()?
        } else {
            let rows = stmt.query_map(params![limit], map_task)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        // Post-filter by role and capabilities since those require enum parsing
        let filtered = tasks
            .into_iter()
            .filter(|task| {
                if let Some(role_filter) = role {
                    if let Some(required_role) = &task.required_role {
                        if required_role.to_string() != role_filter {
                            return false;
                        }
                    }
                }
                capabilities_match(capabilities, &task.required_capabilities)
            })
            .collect();
        Ok(filtered)
    }

    /// List tasks assigned to a specific agent.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_tasks_for_agent(&self, agent_id: &str) -> StoreResult<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT task_id, title, description, requested_by, project_root, parent_task_id,
                   queue_state_id, worktree_binding_id, execution_session_ref, review_cycle_id,
                   workflow_id, phase_id,
                   required_role, required_capabilities, auto_review, verification_required, status,
                   verification_state, priority, severity, owner_agent_id, owner_note,
                   acknowledged_by, acknowledged_at, blocked_reason, verified_by,
                   verified_at, closed_by, closure_summary, closed_at, due_at, review_due_at,
                   scope, created_at, updated_at
            FROM tasks
            WHERE owner_agent_id = ?1
            ORDER BY created_at ASC
            ",
        )?;
        let rows = stmt.query_map([agent_id], map_task)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists direct child tasks for a parent task.
    ///
    /// # Errors
    ///
    /// Returns an error if the parent task does not exist or the query fails.
    pub fn get_children(&self, task_id: &str) -> StoreResult<Vec<TaskSummary>> {
        self.ensure_task_exists(task_id)?;
        let mut stmt = self.conn.prepare(
            r"
            SELECT tasks.task_id, tasks.title, tasks.status
            FROM tasks
            WHERE tasks.parent_task_id = ?1
            ORDER BY tasks.created_at ASC, tasks.task_id ASC
            ",
        )?;
        let rows = stmt.query_map([task_id], |row| {
            Ok(TaskSummary {
                task_id: row.get(0)?,
                title: row.get(1)?,
                status: parse_enum_value::<TaskStatus>(&row.get::<_, String>(2)?, 2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Returns the direct parent task id when one exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn get_parent_id(&self, task_id: &str) -> StoreResult<Option<String>> {
        self.ensure_task_exists(task_id)?;
        self.conn
            .query_row(
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

    pub(crate) fn ensure_task_exists(&self, task_id: &str) -> StoreResult<()> {
        let exists = self
            .conn
            .query_row("SELECT 1 FROM tasks WHERE task_id = ?1", [task_id], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?;
        exists.ok_or(StoreError::NotFound("task"))?;
        Ok(())
    }

    /// Lists open child tasks for a given parent task.
    ///
    /// Returns a vec of (task_id, title, status) tuples for all direct child
    /// tasks that are in an open status (not completed, closed, or cancelled).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_open_child_tasks(&self, parent_task_id: &str) -> StoreResult<Vec<(String, String, TaskStatus)>> {
        // Single query instead of get_children + in-process filter to avoid
        // the extra round-trip of ensure_task_exists + full children fetch.
        let mut stmt = self.conn.prepare(
            r"
            SELECT task_id, title, status
            FROM tasks
            WHERE parent_task_id = ?1
              AND status NOT IN ('completed', 'closed', 'cancelled')
            ORDER BY created_at ASC, task_id ASC
            ",
        )?;
        let rows = stmt.query_map([parent_task_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (id, title, status_str) = row?;
            let status = parse_enum_value::<TaskStatus>(&status_str, 2)?;
            result.push((id, title, status));
        }
        Ok(result)
    }
}

/// Returns `true` when `err` is a `SQLite` UNIQUE constraint violation.
///
/// Used by [`Store::enqueue_task`] to distinguish duplicate-scope rejections
/// from unrelated database failures.
fn is_unique_constraint_error(err: &StoreError) -> bool {
    match err {
        StoreError::Database(rusqlite::Error::SqliteFailure(failure, _)) => {
            failure.code == ErrorCode::ConstraintViolation
        }
        _ => false,
    }
}
