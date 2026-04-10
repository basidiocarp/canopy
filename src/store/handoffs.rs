use rusqlite::params;

use super::helpers::{
    assign_task_in_connection, create_handoff_in_connection, get_handoff_in_connection,
    get_task_in_connection, handoff_is_expired, map_handoff,
    maybe_create_auto_review_subtasks_in_connection, record_task_event_in_connection,
    touch_task_in_connection,
};
use super::{
    HandoffOperatorActionInput, HandoffTiming, Store, StoreError, StoreResult, TaskEventWrite,
};
use crate::models::{Handoff, HandoffStatus, HandoffType, OperatorActionKind, TaskEventType};

impl Store {
    /// Loads a single handoff by id.
    ///
    /// # Errors
    ///
    /// Returns an error if the handoff does not exist or the query fails.
    pub fn get_handoff(&self, handoff_id: &str) -> StoreResult<Handoff> {
        get_handoff_in_connection(&self.conn, handoff_id)
    }

    /// Creates a handoff attached to an existing task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task or source agent does not exist or if the
    /// database write fails.
    #[allow(clippy::too_many_arguments)]
    pub fn create_handoff(
        &self,
        task_id: &str,
        from_agent_id: &str,
        to_agent_id: &str,
        handoff_type: HandoffType,
        summary: &str,
        requested_action: Option<&str>,
        timing: HandoffTiming<'_>,
    ) -> StoreResult<Handoff> {
        self.in_transaction(|conn| {
            create_handoff_in_connection(
                conn,
                task_id,
                from_agent_id,
                to_agent_id,
                handoff_type,
                summary,
                requested_action,
                timing,
            )
        })
    }

    /// Resolves an existing handoff with a terminal or accepted state.
    ///
    /// # Errors
    ///
    /// Returns an error if the handoff does not exist, the requested status is
    /// unsupported, or the update fails.
    pub fn resolve_handoff(
        &self,
        handoff_id: &str,
        status: HandoffStatus,
        resolved_by: &str,
    ) -> StoreResult<Handoff> {
        self.resolve_handoff_with_actor(handoff_id, status, resolved_by, None)
    }

    /// Resolves an open handoff, optionally attributing the resolution to the
    /// acting target agent.
    ///
    /// # Errors
    ///
    /// Returns an error if the handoff is missing, already resolved, attempts
    /// an invalid status transition, or if the acting agent does not satisfy
    /// the acceptance/rejection invariants.
    pub fn resolve_handoff_with_actor(
        &self,
        handoff_id: &str,
        status: HandoffStatus,
        changed_by: &str,
        acting_agent_id: Option<&str>,
    ) -> StoreResult<Handoff> {
        self.in_transaction(|conn| {
            let handoff = get_handoff_in_connection(conn, handoff_id)?;
            if handoff.status != HandoffStatus::Open {
                return Err(StoreError::Validation(
                    "only open handoffs can be resolved".to_string(),
                ));
            }
            if status == HandoffStatus::Open {
                return Err(StoreError::Validation(
                    "handoff resolution cannot transition back to open".to_string(),
                ));
            }
            if status != HandoffStatus::Expired && handoff_is_expired(&handoff)? {
                return Err(StoreError::Validation(
                    "expired handoffs cannot be resolved to a non-expired status".to_string(),
                ));
            }
            if matches!(status, HandoffStatus::Accepted | HandoffStatus::Rejected) {
                let acting_agent_id = acting_agent_id.ok_or_else(|| {
                    StoreError::Validation(format!(
                        "{status} handoff resolution requires an acting_agent_id"
                    ))
                })?;
                if acting_agent_id != handoff.to_agent_id {
                    return Err(StoreError::Validation(
                        "handoff acceptance and rejection must be recorded by the target agent"
                            .to_string(),
                    ));
                }
            }
            let updated = conn.execute(
                r"
                UPDATE handoffs
                SET status = ?2,
                    updated_at = CURRENT_TIMESTAMP,
                    resolved_at = CASE
                        WHEN ?2 = 'open' THEN NULL
                        ELSE CURRENT_TIMESTAMP
                    END
                WHERE handoff_id = ?1
                ",
                params![handoff_id, status.to_string()],
            )?;
            if updated == 0 {
                return Err(StoreError::NotFound("handoff"));
            }

            if status == HandoffStatus::Accepted
                && handoff.handoff_type == HandoffType::TransferOwnership
            {
                assign_task_in_connection(
                    conn,
                    &handoff.task_id,
                    &handoff.to_agent_id,
                    acting_agent_id.unwrap_or(changed_by),
                    Some("accepted transfer ownership handoff"),
                )?;
            } else {
                touch_task_in_connection(conn, &handoff.task_id)?;
            }
            let task = get_task_in_connection(conn, &handoff.task_id)?;
            let event_actor = acting_agent_id.unwrap_or(changed_by);
            let note = format!(
                "handoff_action=resolve; handoff_id={handoff_id}; status:{}->{}{}",
                handoff.status,
                status,
                if event_actor == changed_by {
                    String::new()
                } else {
                    format!("; changed_by={changed_by}")
                }
            );

            record_task_event_in_connection(
                conn,
                &TaskEventWrite {
                    task_id: &handoff.task_id,
                    event_type: TaskEventType::HandoffUpdated,
                    actor: event_actor,
                    from_status: Some(task.status),
                    to_status: task.status,
                    verification_state: Some(task.verification_state),
                    owner_agent_id: task.owner_agent_id.as_deref(),
                    execution_action: None,
                    execution_duration_seconds: None,
                    note: Some(note.as_str()),
                },
            )?;

            maybe_create_auto_review_subtasks_in_connection(conn, &handoff, status, event_actor)?;

            get_handoff_in_connection(conn, handoff_id)
        })
    }

    /// Applies a handoff-scoped operator action using runtime-owned semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if the action is invalid for handoffs or the write
    /// fails.
    #[allow(clippy::too_many_lines)]
    pub fn apply_handoff_operator_action(
        &self,
        handoff_id: &str,
        action: OperatorActionKind,
        changed_by: &str,
        input: HandoffOperatorActionInput<'_>,
    ) -> StoreResult<Handoff> {
        match action {
            OperatorActionKind::AcceptHandoff => self.resolve_handoff_with_actor(
                handoff_id,
                HandoffStatus::Accepted,
                changed_by,
                input.acting_agent_id,
            ),
            OperatorActionKind::RejectHandoff => self.resolve_handoff_with_actor(
                handoff_id,
                HandoffStatus::Rejected,
                changed_by,
                input.acting_agent_id,
            ),
            OperatorActionKind::CancelHandoff => {
                let _ = input;
                self.resolve_handoff(handoff_id, HandoffStatus::Cancelled, changed_by)
            }
            OperatorActionKind::CompleteHandoff => {
                let _ = input;
                self.resolve_handoff(handoff_id, HandoffStatus::Completed, changed_by)
            }
            OperatorActionKind::ExpireHandoff => {
                let _ = input;
                self.resolve_handoff(handoff_id, HandoffStatus::Expired, changed_by)
            }
            OperatorActionKind::FollowUpHandoff => self.in_transaction(|conn| {
                let handoff = get_handoff_in_connection(conn, handoff_id)?;
                if handoff.status != HandoffStatus::Open {
                    return Err(StoreError::Validation(
                        "only open handoffs can be followed up".to_string(),
                    ));
                }
                if handoff_is_expired(&handoff)? {
                    return Err(StoreError::Validation(
                        "expired handoffs cannot be followed up".to_string(),
                    ));
                }
                conn.execute(
                    r"
                    UPDATE handoffs
                    SET updated_at = CURRENT_TIMESTAMP
                    WHERE handoff_id = ?1
                    ",
                    [handoff_id],
                )?;
                touch_task_in_connection(conn, &handoff.task_id)?;
                let task = get_task_in_connection(conn, &handoff.task_id)?;
                let base_note =
                    format!("handoff_action=follow_up; handoff_id={handoff_id}; refreshed=true");
                let note = input.note.map_or(base_note.clone(), |extra| {
                    format!("{base_note}; note={extra}")
                });
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id: &handoff.task_id,
                        event_type: TaskEventType::HandoffUpdated,
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
                get_handoff_in_connection(conn, handoff_id)
            }),
            OperatorActionKind::SummonCouncilSession
            | OperatorActionKind::AcknowledgeTask
            | OperatorActionKind::UnacknowledgeTask
            | OperatorActionKind::VerifyTask
            | OperatorActionKind::RecordDecision
            | OperatorActionKind::CloseTask
            | OperatorActionKind::ReassignTask
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
            | OperatorActionKind::PostCouncilMessage
            | OperatorActionKind::AttachEvidence
            | OperatorActionKind::CreateFollowUpTask
            | OperatorActionKind::LinkTaskDependency => Err(StoreError::Validation(format!(
                "operator action {action} is not valid for handoffs"
            ))),
        }
    }

    /// Lists handoffs globally or for one task.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_handoffs(&self, task_id: Option<&str>) -> StoreResult<Vec<Handoff>> {
        let mut handoffs = Vec::new();
        if let Some(task_id) = task_id {
            let mut stmt = self.conn.prepare(
                r"
                SELECT handoff_id, task_id, from_agent_id, to_agent_id, handoff_type,
                       summary, requested_action, due_at, expires_at, status, created_at, updated_at, resolved_at
                FROM handoffs
                WHERE task_id = ?1
                ORDER BY rowid
                ",
            )?;
            let rows = stmt.query_map([task_id], map_handoff)?;
            for row in rows {
                handoffs.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                r"
                SELECT handoff_id, task_id, from_agent_id, to_agent_id, handoff_type,
                       summary, requested_action, due_at, expires_at, status, created_at, updated_at, resolved_at
                FROM handoffs
                ORDER BY rowid
                ",
            )?;
            let rows = stmt.query_map([], map_handoff)?;
            for row in rows {
                handoffs.push(row?);
            }
        }
        Ok(handoffs)
    }

    /// Lists active (non-terminal) handoffs, optionally filtered by project.
    ///
    /// Terminal statuses (resolved, expired, cancelled) are excluded. When
    /// `project_root` is set, only handoffs whose task belongs to that project
    /// are returned.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_active_handoffs(&self, project_root: Option<&str>) -> StoreResult<Vec<Handoff>> {
        if let Some(project_root) = project_root {
            let mut stmt = self.conn.prepare(
                r"
                SELECT h.handoff_id, h.task_id, h.from_agent_id, h.to_agent_id, h.handoff_type,
                       h.summary, h.requested_action, h.due_at, h.expires_at, h.status, h.created_at, h.updated_at, h.resolved_at
                FROM handoffs h
                JOIN tasks t ON t.task_id = h.task_id
                WHERE t.project_root = ?1
                  AND h.status NOT IN ('rejected', 'expired', 'cancelled', 'completed')
                ORDER BY h.rowid
                ",
            )?;
            let rows = stmt.query_map([project_root], map_handoff)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::from)
        } else {
            let mut stmt = self.conn.prepare(
                r"
                SELECT handoff_id, task_id, from_agent_id, to_agent_id, handoff_type,
                       summary, requested_action, due_at, expires_at, status, created_at, updated_at, resolved_at
                FROM handoffs
                WHERE status NOT IN ('rejected', 'expired', 'cancelled', 'completed')
                ORDER BY rowid
                ",
            )?;
            let rows = stmt.query_map([], map_handoff)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::from)
        }
    }

    /// Lists all handoffs for tasks belonging to a project.
    ///
    /// When `project_root` is `None`, all handoffs are returned (equivalent to
    /// [`list_handoffs`](Self::list_handoffs) with `None`).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_handoffs_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<Handoff>> {
        if let Some(project_root) = project_root {
            let mut stmt = self.conn.prepare(
                r"
                SELECT h.handoff_id, h.task_id, h.from_agent_id, h.to_agent_id, h.handoff_type,
                       h.summary, h.requested_action, h.due_at, h.expires_at, h.status, h.created_at, h.updated_at, h.resolved_at
                FROM handoffs h
                JOIN tasks t ON t.task_id = h.task_id
                WHERE t.project_root = ?1
                ORDER BY h.rowid
                ",
            )?;
            let rows = stmt.query_map([project_root], map_handoff)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::from)
        } else {
            self.list_handoffs(None)
        }
    }

    /// List open handoffs addressed to a specific agent.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_pending_handoffs_for(&self, agent_id: &str) -> StoreResult<Vec<Handoff>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT handoff_id, task_id, from_agent_id, to_agent_id, handoff_type,
                   summary, requested_action, due_at, expires_at, status, created_at,
                   updated_at, resolved_at
            FROM handoffs
            WHERE to_agent_id = ?1 AND status = 'open'
            ORDER BY created_at ASC
            ",
        )?;
        let rows = stmt.query_map([agent_id], map_handoff)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }
}
