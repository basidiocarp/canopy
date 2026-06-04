use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use std::collections::HashMap;

use super::helpers::{
    assign_task_in_connection, create_task_in_connection, get_task_in_connection,
    has_passing_script_verification_in_connection, is_open_task_status,
    list_open_children_in_connection, map_task, maybe_auto_complete_task_tree_in_connection,
    record_parent_relationship_in_connection, record_task_event_in_connection,
    sync_owner_for_task_status, sync_task_workflow_by_id_in_connection,
    sync_task_workflow_in_connection,
};
use super::operator_actions::{
    task_operator_deadline_update, task_operator_status_update, task_operator_triage_update,
};
use super::{
    Store, StoreError, StoreResult, TaskCreationOptions, TaskDeadlineUpdate, TaskEventWrite,
    TaskOperatorActionInput, TaskStatusUpdate, TaskTriageUpdate,
};
use crate::models::{
    AgentRole, HandoffStatus, HandoffType, Notification, NotificationEventType, OperatorActionKind,
    Task, TaskAction, TaskEventType, TaskRelationship, TaskRelationshipRole, TaskStatus,
    TaskSummary, VerificationState, capabilities_match, derive_review_cycle_context,
};

use super::helpers::{handoff_is_expired, parse_enum_value};

#[must_use]
pub fn compute_body_hash(title: &str, description: Option<&str>, scope: &[String]) -> String {
    // FNV-1a: deterministic across compiler versions and platforms.
    // std DefaultHasher is explicitly not guaranteed stable across Rust releases.
    const PRIME: u64 = 1_099_511_628_211;
    const OFFSET: u64 = 14_695_981_039_346_656_037;
    let mut hash = OFFSET;
    let bytes = title
        .bytes()
        .chain(std::iter::once(b'\x00'))
        .chain(description.unwrap_or("").bytes())
        .chain(std::iter::once(b'\x00'))
        .chain(
            scope
                .iter()
                .flat_map(|s| s.bytes().chain(std::iter::once(b'\x00'))),
        );
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}

/// Records the body hash on first dispatch for tasks that track plan immutability.
///
/// Both `atomic_claim_task` variants bypass `update_task_status`, so they call
/// this helper directly after their raw SQL UPDATE succeeds.
fn record_body_hash_if_needed(conn: &rusqlite::Connection, task_id: &str) -> StoreResult<()> {
    let task = get_task_in_connection(conn, task_id)?;
    if task.immutable_once_dispatched && task.body_hash.is_none() {
        let hash = compute_body_hash(&task.title, task.description.as_deref(), &task.scope);
        conn.execute(
            "UPDATE tasks SET body_hash = ?2, updated_at = CURRENT_TIMESTAMP WHERE task_id = ?1",
            params![task_id, hash],
        )?;
    }
    Ok(())
}

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

    /// Enqueues a new scoped task, preventing duplicates across non-terminal states.
    ///
    /// When `scope` is non-empty, this function checks for any existing task with
    /// the same scope that is NOT in a terminal state (`open`, `assigned`, `in_progress`,
    /// `blocked`, `review_required`). If such a task is found, returns
    /// [`StoreError::DuplicateQueuedTask`] with the blocking task's ID and status.
    ///
    /// If a scoped task is found in a terminal state (`completed`, `closed`, `cancelled`),
    /// creation is allowed, and the created task's `prior_task_id` is set to point
    /// to the most recently completed task, enabling idempotent rediscovery of work.
    ///
    /// Tasks with an empty scope (`options.scope` is empty) are created without
    /// uniqueness enforcement — multiple unscoped tasks can coexist.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::DuplicateQueuedTask`] when a non-terminal task for the
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
        if options.scope.is_empty() {
            // Unscoped task; create directly without checks.
            self.create_task_with_options(title, description, requested_by, project_root, options)
        } else {
            // Check for any existing task with the same scope in a non-terminal state.
            let scope_json = serde_json::to_string(&options.scope)
                .map_err(|e| StoreError::Validation(format!("failed to serialize scope: {e}")))?;

            self.in_transaction(|conn| {
                let mut stmt = conn.prepare(
                    r"
                    SELECT task_id, status
                    FROM tasks
                    WHERE scope = ?1
                    ORDER BY created_at DESC, task_id DESC
                    ",
                )?;
                let mut rows = stmt.query([&scope_json])?;

                let mut most_recent_terminal: Option<String> = None;

                while let Some(row) = rows.next()? {
                    let task_id: String = row.get(0)?;
                    let status_str: String = row.get(1)?;
                    let status = parse_enum_value::<TaskStatus>(&status_str, 1)?;

                    if is_open_task_status(status) {
                        // Found a non-terminal task blocking this scope.
                        let scope_display = options.scope.join(", ");
                        return Err(StoreError::DuplicateQueuedTask {
                            scope: scope_display,
                        });
                    }

                    // Found a terminal task; remember it (the first/most recent one).
                    if most_recent_terminal.is_none() {
                        most_recent_terminal = Some(task_id);
                    }
                }

                // No blocking tasks found; proceed with creation.
                let mut created = create_task_in_connection(
                    conn,
                    title,
                    description,
                    requested_by,
                    project_root,
                    options,
                )?;

                // If we found a recently completed task, link it as prior_task_id.
                if let Some(prior_id) = most_recent_terminal {
                    created.prior_task_id = Some(prior_id);
                }

                Ok(created)
            })
        }
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
        force: bool,
    ) -> StoreResult<Task> {
        self.ensure_agent_exists(assigned_to)?;
        self.ensure_task_exists(task_id)?;
        self.in_transaction(|conn| {
            assign_task_in_connection(conn, task_id, assigned_to, assigned_by, reason, force)?;
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
            SELECT task_id, title, description, requested_by, project_root, workspace, parent_task_id,
                   queue_state_id, worktree_binding_id, execution_session_ref, review_cycle_id,
                   workflow_id, phase_id,
                   required_role, required_capabilities, auto_review, verification_required, status, verification_state, priority, severity, owner_agent_id, owner_note,
                   acknowledged_by, acknowledged_at, blocked_reason, verified_by,
                   verified_at, closed_by, closure_summary, closed_at, due_at, review_due_at,
                   scope, created_at, updated_at, immutable_once_dispatched, body_hash,
                   branch_of, branch_at, branch_outcome, score, score_reasons, contract_path
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
    /// When updating a terminal status (completed, closed, cancelled) to the same status,
    /// this is a no-op: the task is returned without updating the row or emitting an event.
    /// This ensures idempotent terminal state writes.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist, the requested transition is
    /// invalid, or the update fails.
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

            // Detect silent body rewrites: if a hash was locked at dispatch time,
            // compare it against the current body. A mismatch means the title,
            // description, or scope was changed after dispatch without going through
            // a sanctioned update path.
            if current.immutable_once_dispatched {
                if let Some(ref stored) = current.body_hash {
                    let live = compute_body_hash(
                        &current.title,
                        current.description.as_deref(),
                        &current.scope,
                    );
                    if *stored != live {
                        tracing::warn!(
                            task_id = %task_id,
                            stored_hash = %stored,
                            live_hash = %live,
                            "plan immutability violation: task body was rewritten after dispatch"
                        );
                    }
                }
            }

            let from_status = current.status;
            let next_verification = update
                .verification_state
                .unwrap_or(current.verification_state);

            let is_terminal = matches!(
                status,
                TaskStatus::Completed | TaskStatus::Closed | TaskStatus::Cancelled
            );

            // Idempotent terminal state writes: no-op if already in this terminal state.
            if is_terminal && from_status == status {
                return Ok(current);
            }

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
                if super::helpers::has_open_child_tasks_in_connection(conn, task_id)? {
                    let open_children = list_open_children_in_connection(conn, task_id)?;
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

            // Compute body hash if transitioning to in_progress or assigned and hash not yet recorded
            let body_hash = if (status == TaskStatus::InProgress || status == TaskStatus::Assigned)
                && current.body_hash.is_none()
                && current.immutable_once_dispatched
            {
                Some(compute_body_hash(
                    &current.title,
                    current.description.as_deref(),
                    &current.scope,
                ))
            } else {
                None
            };

            // When reopening from a terminal state (Completed, Closed, Cancelled), clear verification metadata.
            // Non-terminal→Open transitions (e.g., Blocked→Open) preserve verification metadata.
            let clear_from_terminal = status == TaskStatus::Open && current.status.is_terminal();

            conn.execute(
                r"
                UPDATE tasks
                SET status = ?2,
                    verification_state = ?3,
                    blocked_reason = ?4,
                    verified_by = CASE WHEN ?11 THEN NULL ELSE ?5 END,
                    verified_at = CASE WHEN ?11 THEN NULL ELSE COALESCE(?6, verified_at) END,
                    owner_agent_id = CASE WHEN ?11 THEN NULL ELSE owner_agent_id END,
                    closed_by = ?7,
                    closure_summary = ?8,
                    closed_at = CASE WHEN ?9 THEN CURRENT_TIMESTAMP ELSE NULL END,
                    body_hash = COALESCE(?10, body_hash),
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
                    body_hash,
                    clear_from_terminal,
                ],
            )?;

            sync_owner_for_task_status(conn, task_id, status)?;
            sync_task_workflow_by_id_in_connection(conn, task_id)?;

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
                // Notification failure must not fail the task update, but log so it is visible.
                if let Err(e) = super::notifications::insert_notification(conn, &notif) {
                    tracing::warn!(error = %e, "notification insert failed — task update succeeded");
                }
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
            // Resync queue state after triage changes (priority/severity may affect queue position).
            sync_task_workflow_in_connection(conn, &updated)?;
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
                // Hoist list_related_tasks and list_handoffs once so both
                // graph-pressure and handoff-follow-through checks share the same fetch.
                let related = self.list_related_tasks(task_id)?;
                let handoffs_for_task = self.list_handoffs(Some(task_id))?;
                if related.iter().any(|r| {
                    (r.relationship_role == TaskRelationshipRole::BlockedBy
                        && matches!(
                            r.status,
                            TaskStatus::Open
                                | TaskStatus::Assigned
                                | TaskStatus::InProgress
                                | TaskStatus::Blocked
                                | TaskStatus::ReviewRequired
                        ))
                        || (r.relationship_role == TaskRelationshipRole::FollowUpChild
                            && matches!(
                                r.status,
                                TaskStatus::Open
                                    | TaskStatus::Assigned
                                    | TaskStatus::InProgress
                                    | TaskStatus::Blocked
                                    | TaskStatus::ReviewRequired
                            ))
                        || (r.relationship_role == TaskRelationshipRole::Child
                            && matches!(
                                r.status,
                                TaskStatus::Open
                                    | TaskStatus::Assigned
                                    | TaskStatus::InProgress
                                    | TaskStatus::Blocked
                                    | TaskStatus::ReviewRequired
                            ))
                }) {
                    return Err(StoreError::Validation(
                        "close_task requires review tasks without unresolved graph pressure"
                            .to_string(),
                    ));
                }
                if handoffs_for_task.into_iter().any(|handoff| {
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
                }) {
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
                input.force_reassign,
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
            | OperatorActionKind::AttachReviewAnnotation
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
            SELECT task_id, title, description, requested_by, project_root, workspace, parent_task_id,
                   queue_state_id, worktree_binding_id, execution_session_ref, review_cycle_id,
                   workflow_id, phase_id,
                   required_role, required_capabilities, auto_review, verification_required, status, verification_state, priority, severity, owner_agent_id, owner_note,
                   acknowledged_by, acknowledged_at, blocked_reason, verified_by,
                   verified_at, closed_by, closure_summary, closed_at, due_at, review_due_at,
                   scope, created_at, updated_at, immutable_once_dispatched, body_hash,
                   branch_of, branch_at, branch_outcome, score, score_reasons, contract_path
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
            sync_task_workflow_by_id_in_connection(conn, task_id)?;
            Ok(())
        })
    }

    /// Updates task score and appends a reason to the `score_reasons` list.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub fn score_task(&self, task_id: &str, score: f64, reason: &str) -> StoreResult<Task> {
        if !score.is_finite() {
            return Err(StoreError::Validation(
                "score must be a finite number".to_owned(),
            ));
        }
        self.in_transaction(|conn| {
            let task = get_task_in_connection(conn, task_id)?;
            let mut reasons = task.score_reasons;
            if !reason.is_empty() {
                reasons.push(reason.to_owned());
            }
            let reasons_json = serde_json::to_string(&reasons).unwrap_or_else(|_| "[]".to_owned());
            conn.execute(
                "UPDATE tasks SET score = ?2, score_reasons = ?3, updated_at = CURRENT_TIMESTAMP WHERE task_id = ?1",
                params![task_id, score, reasons_json],
            )?;
            get_task_in_connection(conn, task_id)
        })
    }

    /// Set the `contract_path` on a task.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub fn set_contract_path(&self, task_id: &str, contract_path: &str) -> StoreResult<Task> {
        self.in_transaction(|conn| {
            conn.execute(
                "UPDATE tasks SET contract_path = ?2, updated_at = CURRENT_TIMESTAMP WHERE task_id = ?1",
                params![task_id, contract_path],
            )?;
            get_task_in_connection(conn, task_id)
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
                // Lock the body hash on first dispatch. This path bypasses
                // update_task_status, so we record it here explicitly.
                record_body_hash_if_needed(conn, task_id)?;
                sync_task_workflow_by_id_in_connection(conn, task_id)?;
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
                // Lock the body hash on first dispatch. This path bypasses
                // update_task_status, so we record it here explicitly.
                record_body_hash_if_needed(conn, task_id)?;
                sync_task_workflow_by_id_in_connection(conn, task_id)?;
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
            SELECT task_id, title, description, requested_by, project_root, workspace, parent_task_id,
                   queue_state_id, worktree_binding_id, execution_session_ref, review_cycle_id,
                   workflow_id, phase_id,
                   required_role, required_capabilities, auto_review, verification_required, status,
                   verification_state, priority, severity, owner_agent_id, owner_note,
                   acknowledged_by, acknowledged_at, blocked_reason, verified_by,
                   verified_at, closed_by, closure_summary, closed_at, due_at, review_due_at,
                   scope, created_at, updated_at, immutable_once_dispatched, body_hash,
                   branch_of, branch_at, branch_outcome, score, score_reasons, contract_path
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

        // Parse the role filter string once rather than calling to_string() on
        // every task's required_role inside the closure.
        let role_filter: Option<AgentRole> = role.and_then(|r| r.parse().ok());

        // Post-filter by role and capabilities since those require enum parsing
        let filtered = tasks
            .into_iter()
            .filter(|task| {
                if let Some(r) = role_filter {
                    if task.required_role != Some(r) {
                        return false;
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
            SELECT task_id, title, description, requested_by, project_root, workspace, parent_task_id,
                   queue_state_id, worktree_binding_id, execution_session_ref, review_cycle_id,
                   workflow_id, phase_id,
                   required_role, required_capabilities, auto_review, verification_required, status,
                   verification_state, priority, severity, owner_agent_id, owner_note,
                   acknowledged_by, acknowledged_at, blocked_reason, verified_by,
                   verified_at, closed_by, closure_summary, closed_at, due_at, review_due_at,
                   scope, created_at, updated_at, immutable_once_dispatched, body_hash,
                   branch_of, branch_at, branch_outcome, score, score_reasons, contract_path
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
    /// Returns a vec of (`task_id`, title, status) tuples for all direct child
    /// tasks that are in an open status (not completed, closed, or cancelled).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_open_child_tasks(
        &self,
        parent_task_id: &str,
    ) -> StoreResult<Vec<(String, String, TaskStatus)>> {
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

    /// Persist structured task output to a task's output column.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub fn set_task_output(&self, task_id: &str, output_json: &str) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE tasks SET output = ?1, updated_at = CURRENT_TIMESTAMP WHERE task_id = ?2",
            params![output_json, task_id],
        )?;
        Ok(())
    }

    /// Retrieve structured task output from a task's output column.
    ///
    /// Returns `None` if output is NULL.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub fn get_task_output(&self, task_id: &str) -> StoreResult<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT output FROM tasks WHERE task_id = ?1")?;
        let result = stmt
            .query_row([task_id], |row| row.get::<_, Option<String>>(0))
            .optional()?
            .flatten();
        Ok(result)
    }

    /// Persist the agent's completion signal JSON to a task's
    /// `completion_signal` column.
    ///
    /// The stored value is the serialized `canopy-task-completion-signal-v1`
    /// payload built at completion time; it is surfaced unchanged on the
    /// task-detail read model so the hymenium dispatch loop can read it.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub fn set_completion_signal(&self, task_id: &str, signal_json: &str) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE tasks SET completion_signal = ?1, updated_at = CURRENT_TIMESTAMP WHERE task_id = ?2",
            params![signal_json, task_id],
        )?;
        Ok(())
    }

    /// Retrieve the persisted completion signal JSON from a task's
    /// `completion_signal` column.
    ///
    /// Returns `None` if the task has not been completed (column is NULL).
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub fn get_completion_signal(&self, task_id: &str) -> StoreResult<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT completion_signal FROM tasks WHERE task_id = ?1")?;
        let result = stmt
            .query_row([task_id], |row| row.get::<_, Option<String>>(0))
            .optional()?
            .flatten();
        Ok(result)
    }

    /// Delete a task and all its related records via foreign-key cascade.
    ///
    /// Tables with `ON DELETE CASCADE` on `task_id` are removed automatically
    /// by the `SQLite` FK engine when `PRAGMA foreign_keys = ON` is active.
    /// This includes `task_queue_states`, `task_worktree_bindings`,
    /// `task_review_cycles`, `task_assignments`, `handoffs`,
    /// `council_messages`, `council_sessions`, `evidence_refs`, `task_events`,
    /// `task_relationships`, `file_locks`, `tool_adoption_scores`, and
    /// `notifications`. All active file locks for the task are released
    /// atomically as part of this cascade.
    ///
    /// This is a destructive operation intended only for rollback scenarios
    /// (e.g., cleaning up a partially-created task tree). Prefer status
    /// transitions (`cancelled`, `closed`) for normal lifecycle management.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub fn delete_task(&self, task_id: &str) -> StoreResult<()> {
        self.conn
            .execute("DELETE FROM tasks WHERE task_id = ?1", params![task_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentRegistration, AgentStatus};
    use tempfile::tempdir;

    fn create_test_store() -> StoreResult<Store> {
        let tmpdir = tempdir().expect("failed to create tmpdir");
        let db_path = tmpdir.path().join("test.db");
        Store::open(&db_path)
    }

    fn register_test_agent(store: &Store, agent_id: &str) -> StoreResult<()> {
        let agent = AgentRegistration {
            agent_id: agent_id.to_string(),
            host_id: "test_host".to_string(),
            host_type: "test".to_string(),
            host_instance: "test_instance".to_string(),
            model: "test_model".to_string(),
            project_root: "/test".to_string(),
            worktree_id: "test_worktree".to_string(),
            role: None,
            capabilities: vec![],
            tier: None,
            specializations: vec![],
            status: AgentStatus::Idle,
            current_task_id: None,
            heartbeat_at: Some(chrono::Utc::now().to_rfc3339()),
        };
        store.register_agent(&agent)?;
        Ok(())
    }

    #[test]
    fn test_score_task_sets_score_and_appends_reason() -> StoreResult<()> {
        let store = create_test_store()?;
        let agent_id = "test_agent";
        register_test_agent(&store, agent_id)?;

        let task = store.create_task("Score Task", None, agent_id, "/test", None)?;

        let scored = store.score_task(&task.task_id, 0.8, "first reason")?;
        assert_eq!(scored.score, Some(0.8));
        assert_eq!(scored.score_reasons, vec!["first reason"]);

        // A second call with a different reason appends, not replaces.
        let scored2 = store.score_task(&task.task_id, 0.9, "second reason")?;
        assert_eq!(scored2.score, Some(0.9));
        assert_eq!(scored2.score_reasons, vec!["first reason", "second reason"]);

        Ok(())
    }

    #[test]
    fn test_score_task_not_found_returns_error() -> StoreResult<()> {
        let store = create_test_store()?;

        let result = store.score_task("nonexistent-task-id", 0.5, "reason");
        assert!(
            matches!(result, Err(StoreError::NotFound(_))),
            "expected NotFound, got {result:?}"
        );

        Ok(())
    }

    #[test]
    fn test_score_task_empty_reason_not_appended() -> StoreResult<()> {
        let store = create_test_store()?;
        let agent_id = "test_agent";
        register_test_agent(&store, agent_id)?;

        let task = store.create_task("Score Task Empty Reason", None, agent_id, "/test", None)?;

        let scored = store.score_task(&task.task_id, 0.5, "")?;
        assert_eq!(scored.score, Some(0.5));
        assert!(
            scored.score_reasons.is_empty(),
            "empty reason should not be appended"
        );

        Ok(())
    }

    #[test]
    fn test_score_task_nan_returns_error() -> StoreResult<()> {
        let store = create_test_store()?;
        let agent_id = "test_agent";
        register_test_agent(&store, agent_id)?;

        let task = store.create_task("Score Task NaN", None, agent_id, "/test", None)?;

        let result = store.score_task(&task.task_id, f64::NAN, "nan reason");
        assert!(
            matches!(result, Err(StoreError::Validation(_))),
            "expected Validation error for NaN score, got {result:?}"
        );

        Ok(())
    }

    #[test]
    fn test_reopen_completed_task_clears_verification_metadata() -> StoreResult<()> {
        let store = create_test_store()?;
        let agent_id = "test_agent";
        register_test_agent(&store, agent_id)?;

        let task = store.create_task(
            "Test Task",
            Some("Test description"),
            agent_id,
            "/test",
            None,
        )?;

        // Assign the task to set owner_agent_id
        store.assign_task(&task.task_id, agent_id, agent_id, None, false)?;

        // Transition: Assigned → InProgress → Completed
        store.update_task_status(
            &task.task_id,
            TaskStatus::InProgress,
            agent_id,
            TaskStatusUpdate {
                verification_state: None,
                blocked_reason: None,
                closure_summary: None,
                event_note: None,
            },
        )?;

        let completed = store.update_task_status(
            &task.task_id,
            TaskStatus::Completed,
            agent_id,
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Passed),
                blocked_reason: None,
                closure_summary: Some("Task completed successfully"),
                event_note: None,
            },
        )?;

        // Verify that completion set the metadata
        assert!(completed.verified_by.is_some());
        assert!(completed.verified_at.is_some());
        assert_eq!(completed.owner_agent_id, Some(agent_id.to_string()));

        // Reopen the task from Completed
        let reopened = store.update_task_status(
            &task.task_id,
            TaskStatus::Open,
            agent_id,
            TaskStatusUpdate {
                verification_state: None,
                blocked_reason: None,
                closure_summary: None,
                event_note: Some("Reopening from completed"),
            },
        )?;

        // Verify that reopening cleared the verification metadata
        assert!(
            reopened.verified_by.is_none(),
            "verified_by should be cleared"
        );
        assert!(
            reopened.verified_at.is_none(),
            "verified_at should be cleared"
        );
        assert!(
            reopened.owner_agent_id.is_none(),
            "owner_agent_id should be cleared"
        );
        assert_eq!(reopened.status, TaskStatus::Open);

        Ok(())
    }

    #[test]
    fn test_reopen_closed_task_clears_verification_metadata() -> StoreResult<()> {
        let store = create_test_store()?;
        let agent_id = "test_agent";
        register_test_agent(&store, agent_id)?;

        let task = store.create_task(
            "Test Task",
            Some("Test description"),
            agent_id,
            "/test",
            None,
        )?;

        // Assign the task to set owner_agent_id
        store.assign_task(&task.task_id, agent_id, agent_id, None, false)?;

        // Transition: Assigned → InProgress → Completed → Closed
        store.update_task_status(
            &task.task_id,
            TaskStatus::InProgress,
            agent_id,
            TaskStatusUpdate {
                verification_state: None,
                blocked_reason: None,
                closure_summary: None,
                event_note: None,
            },
        )?;

        store.update_task_status(
            &task.task_id,
            TaskStatus::Completed,
            agent_id,
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Passed),
                blocked_reason: None,
                closure_summary: Some("Completed"),
                event_note: None,
            },
        )?;

        let closed = store.update_task_status(
            &task.task_id,
            TaskStatus::Closed,
            agent_id,
            TaskStatusUpdate {
                verification_state: None,
                blocked_reason: None,
                closure_summary: Some("Closed for archival"),
                event_note: None,
            },
        )?;

        // Verify closure metadata is present
        assert!(closed.verified_by.is_some());
        assert!(closed.verified_at.is_some());

        // Reopen from closed
        let reopened = store.update_task_status(
            &task.task_id,
            TaskStatus::Open,
            agent_id,
            TaskStatusUpdate {
                verification_state: None,
                blocked_reason: None,
                closure_summary: None,
                event_note: Some("Reopening from closed"),
            },
        )?;

        // Verify that reopening cleared the metadata
        assert!(reopened.verified_by.is_none());
        assert!(reopened.verified_at.is_none());
        assert!(reopened.owner_agent_id.is_none());

        Ok(())
    }

    #[test]
    fn test_reopen_blocked_task_preserves_verification_metadata() -> StoreResult<()> {
        let store = create_test_store()?;
        let agent_id = "test_agent";
        register_test_agent(&store, agent_id)?;

        let task = store.create_task(
            "Test Task",
            Some("Test description"),
            agent_id,
            "/test",
            None,
        )?;

        // Assign the task
        let _assigned = store.update_task_status(
            &task.task_id,
            TaskStatus::Assigned,
            agent_id,
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Passed),
                blocked_reason: None,
                closure_summary: None,
                event_note: None,
            },
        )?;

        // Block the task
        let _blocked = store.update_task_status(
            &task.task_id,
            TaskStatus::Blocked,
            agent_id,
            TaskStatusUpdate {
                verification_state: None,
                blocked_reason: Some("Waiting for dependency"),
                closure_summary: None,
                event_note: None,
            },
        )?;

        // Reopen from blocked (non-terminal)
        let reopened = store.update_task_status(
            &task.task_id,
            TaskStatus::Open,
            agent_id,
            TaskStatusUpdate {
                verification_state: None,
                blocked_reason: None,
                closure_summary: None,
                event_note: Some("Dependency resolved"),
            },
        )?;

        // Verify that reopening from blocked PRESERVES metadata (non-terminal transition)
        assert_eq!(
            reopened.verified_by,
            Some(agent_id.to_string()),
            "verified_by should be preserved for non-terminal reopening"
        );
        assert!(
            reopened.verified_at.is_some(),
            "verified_at should be preserved for non-terminal reopening"
        );
        // owner_agent_id is cleared when exiting Blocked, not affected by this change
        assert_eq!(reopened.status, TaskStatus::Open);

        Ok(())
    }

    #[test]
    fn test_reopen_cancelled_task_clears_verification_metadata() -> StoreResult<()> {
        let store = create_test_store()?;
        let agent_id = "test_agent";
        register_test_agent(&store, agent_id)?;

        let task = store.create_task(
            "Test Task",
            Some("Test description"),
            agent_id,
            "/test",
            None,
        )?;

        // Assign, then cancel the task
        let _assigned = store.update_task_status(
            &task.task_id,
            TaskStatus::Assigned,
            agent_id,
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Passed),
                blocked_reason: None,
                closure_summary: None,
                event_note: None,
            },
        )?;

        let cancelled = store.update_task_status(
            &task.task_id,
            TaskStatus::Cancelled,
            agent_id,
            TaskStatusUpdate {
                verification_state: None,
                blocked_reason: None,
                closure_summary: Some("Task cancelled"),
                event_note: None,
            },
        )?;

        // Verify that cancellation set the metadata
        assert!(cancelled.verified_by.is_some());
        assert!(cancelled.verified_at.is_some());

        // Reopen from cancelled
        let reopened = store.update_task_status(
            &task.task_id,
            TaskStatus::Open,
            agent_id,
            TaskStatusUpdate {
                verification_state: None,
                blocked_reason: None,
                closure_summary: None,
                event_note: Some("Reopening from cancelled"),
            },
        )?;

        // Verify that reopening cleared the metadata
        assert!(reopened.verified_by.is_none());
        assert!(reopened.verified_at.is_none());
        assert!(reopened.owner_agent_id.is_none());

        Ok(())
    }
}
