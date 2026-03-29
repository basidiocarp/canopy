use crate::models::{
    AgentHeartbeatEvent, AgentHeartbeatSource, AgentRegistration, AgentStatus, CouncilMessage,
    CouncilMessageType, EvidenceRef, EvidenceSourceKind, Handoff, HandoffStatus, HandoffType,
    OperatorActionKind, RelatedTask, Task, TaskAssignment, TaskEvent, TaskEventType, TaskPriority,
    TaskRelationship, TaskRelationshipKind, TaskRelationshipRole, TaskSeverity, TaskStatus,
    VerificationState,
};
use rusqlite::{Connection, OptionalExtension, params, types::Type};
use std::fs;
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use ulid::Ulid;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("record not found: {0}")]
    NotFound(&'static str),
    #[error("validation error: {0}")]
    Validation(String),
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

#[derive(Debug)]
struct TaskEventWrite<'a> {
    task_id: &'a str,
    event_type: TaskEventType,
    actor: &'a str,
    from_status: Option<TaskStatus>,
    to_status: TaskStatus,
    verification_state: Option<VerificationState>,
    owner_agent_id: Option<&'a str>,
    note: Option<&'a str>,
}

#[derive(Debug)]
struct AgentHeartbeatWrite<'a> {
    agent_id: &'a str,
    status: AgentStatus,
    current_task_id: Option<&'a str>,
    related_task_id: Option<&'a str>,
    source: AgentHeartbeatSource,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EvidenceLinkRefs<'a> {
    pub related_handoff_id: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub memory_query: Option<&'a str>,
    pub symbol: Option<&'a str>,
    pub file: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TaskTriageUpdate<'a> {
    pub priority: Option<TaskPriority>,
    pub severity: Option<TaskSeverity>,
    pub acknowledged: Option<bool>,
    pub owner_note: Option<&'a str>,
    pub clear_owner_note: bool,
    pub event_note: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HandoffTiming<'a> {
    pub due_at: Option<&'a str>,
    pub expires_at: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TaskStatusUpdate<'a> {
    pub verification_state: Option<VerificationState>,
    pub blocked_reason: Option<&'a str>,
    pub closure_summary: Option<&'a str>,
    pub event_note: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TaskOperatorActionInput<'a> {
    pub assigned_to: Option<&'a str>,
    pub priority: Option<TaskPriority>,
    pub severity: Option<TaskSeverity>,
    pub verification_state: Option<VerificationState>,
    pub blocked_reason: Option<&'a str>,
    pub closure_summary: Option<&'a str>,
    pub owner_note: Option<&'a str>,
    pub clear_owner_note: bool,
    pub note: Option<&'a str>,
    pub from_agent_id: Option<&'a str>,
    pub to_agent_id: Option<&'a str>,
    pub handoff_type: Option<HandoffType>,
    pub handoff_summary: Option<&'a str>,
    pub requested_action: Option<&'a str>,
    pub due_at: Option<&'a str>,
    pub expires_at: Option<&'a str>,
    pub author_agent_id: Option<&'a str>,
    pub message_type: Option<CouncilMessageType>,
    pub message_body: Option<&'a str>,
    pub evidence_source_kind: Option<EvidenceSourceKind>,
    pub evidence_source_ref: Option<&'a str>,
    pub evidence_label: Option<&'a str>,
    pub evidence_summary: Option<&'a str>,
    pub related_handoff_id: Option<&'a str>,
    pub related_session_id: Option<&'a str>,
    pub related_memory_query: Option<&'a str>,
    pub related_symbol: Option<&'a str>,
    pub related_file: Option<&'a str>,
    pub follow_up_title: Option<&'a str>,
    pub follow_up_description: Option<&'a str>,
    pub related_task_id: Option<&'a str>,
    pub relationship_role: Option<TaskRelationshipRole>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HandoffOperatorActionInput<'a> {
    pub note: Option<&'a str>,
}

const BASE_SCHEMA: &str = r"
    CREATE TABLE IF NOT EXISTS agents (
        agent_id TEXT PRIMARY KEY,
        host_id TEXT NOT NULL,
        host_type TEXT NOT NULL,
        host_instance TEXT NOT NULL,
        model TEXT NOT NULL,
        project_root TEXT NOT NULL,
        worktree_id TEXT NOT NULL,
        status TEXT NOT NULL,
        current_task_id TEXT NULL,
        heartbeat_at TEXT NULL
    );

    CREATE TABLE IF NOT EXISTS tasks (
        task_id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        description TEXT NULL,
        requested_by TEXT NOT NULL,
        project_root TEXT NOT NULL,
        status TEXT NOT NULL,
        verification_state TEXT NOT NULL,
        priority TEXT NOT NULL,
        severity TEXT NOT NULL,
        owner_agent_id TEXT NULL REFERENCES agents(agent_id),
        owner_note TEXT NULL,
        acknowledged_by TEXT NULL,
        acknowledged_at TEXT NULL,
        blocked_reason TEXT NULL,
        verified_by TEXT NULL,
        verified_at TEXT NULL,
        closed_by TEXT NULL,
        closure_summary TEXT NULL,
        closed_at TEXT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS task_assignments (
        assignment_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        assigned_to TEXT NOT NULL REFERENCES agents(agent_id),
        assigned_by TEXT NOT NULL,
        reason TEXT NULL,
        assigned_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS handoffs (
        handoff_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        from_agent_id TEXT NOT NULL REFERENCES agents(agent_id),
        to_agent_id TEXT NOT NULL REFERENCES agents(agent_id),
        handoff_type TEXT NOT NULL,
        summary TEXT NOT NULL,
        requested_action TEXT NULL,
        due_at TEXT NULL,
        expires_at TEXT NULL,
        status TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        resolved_at TEXT NULL
    );

    CREATE TABLE IF NOT EXISTS council_messages (
        message_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        author_agent_id TEXT NOT NULL REFERENCES agents(agent_id),
        message_type TEXT NOT NULL,
        body TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS evidence_refs (
        evidence_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        source_kind TEXT NOT NULL,
        source_ref TEXT NOT NULL,
        label TEXT NOT NULL,
        summary TEXT NULL,
        related_handoff_id TEXT NULL REFERENCES handoffs(handoff_id),
        related_session_id TEXT NULL,
        related_memory_query TEXT NULL,
        related_symbol TEXT NULL,
        related_file TEXT NULL
    );

    CREATE TABLE IF NOT EXISTS task_events (
        event_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        event_type TEXT NOT NULL,
        actor TEXT NOT NULL,
        from_status TEXT NULL,
        to_status TEXT NOT NULL,
        verification_state TEXT NULL,
        owner_agent_id TEXT NULL,
        note TEXT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS task_relationships (
        relationship_id TEXT PRIMARY KEY,
        source_task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        target_task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        kind TEXT NOT NULL,
        created_by TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        UNIQUE(source_task_id, target_task_id, kind)
    );

    CREATE TABLE IF NOT EXISTS agent_heartbeat_events (
        heartbeat_id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL REFERENCES agents(agent_id) ON DELETE CASCADE,
        status TEXT NOT NULL,
        current_task_id TEXT NULL REFERENCES tasks(task_id) ON DELETE SET NULL,
        related_task_id TEXT NULL REFERENCES tasks(task_id) ON DELETE SET NULL,
        source TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
";

impl Store {
    /// Opens the Canopy store and creates the schema when needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the parent directory cannot be created, the
    /// database cannot be opened, or schema initialization fails.
    pub fn open(path: &Path) -> StoreResult<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| StoreError::Validation(error.to_string()))?;
        }

        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(BASE_SCHEMA)?;

        migrate_schema(&conn)?;

        Ok(Self { conn })
    }

    /// Registers or refreshes an agent entry in the local registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying database write fails.
    pub fn register_agent(&self, agent: &AgentRegistration) -> StoreResult<AgentRegistration> {
        self.in_transaction(|conn| {
            validate_agent_registration(conn, agent)?;
            conn.execute(
                r"
                INSERT INTO agents (
                    agent_id, host_id, host_type, host_instance, model,
                    project_root, worktree_id, status, current_task_id, heartbeat_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, CURRENT_TIMESTAMP)
                ON CONFLICT(agent_id) DO UPDATE SET
                    host_id = excluded.host_id,
                    host_type = excluded.host_type,
                    host_instance = excluded.host_instance,
                    model = excluded.model,
                    project_root = excluded.project_root,
                    worktree_id = excluded.worktree_id,
                    status = excluded.status,
                    current_task_id = excluded.current_task_id,
                    heartbeat_at = CURRENT_TIMESTAMP
                ",
                params![
                    agent.agent_id,
                    agent.host_id,
                    agent.host_type,
                    agent.host_instance,
                    agent.model,
                    agent.project_root,
                    agent.worktree_id,
                    agent.status.to_string(),
                    agent.current_task_id,
                ],
            )?;
            record_agent_heartbeat_in_connection(
                conn,
                &AgentHeartbeatWrite {
                    agent_id: &agent.agent_id,
                    status: agent.status,
                    current_task_id: agent.current_task_id.as_deref(),
                    related_task_id: agent.current_task_id.as_deref(),
                    source: AgentHeartbeatSource::Register,
                },
            )?;
            get_agent_in_connection(conn, &agent.agent_id)
        })
    }

    /// Lists the registered agents in stable identifier order.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_agents(&self) -> StoreResult<Vec<AgentRegistration>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT agent_id, host_id, host_type, host_instance, model,
                   project_root, worktree_id, status, current_task_id, heartbeat_at
            FROM agents
            ORDER BY agent_id
            ",
        )?;
        let rows = stmt.query_map([], map_agent)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Updates an agent heartbeat and optional active-task context.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent does not exist or the update fails.
    pub fn heartbeat_agent(
        &self,
        agent_id: &str,
        status: AgentStatus,
        current_task_id: Option<&str>,
    ) -> StoreResult<AgentRegistration> {
        self.ensure_agent_exists(agent_id)?;
        self.in_transaction(|conn| {
            validate_agent_task_link(conn, agent_id, status, current_task_id)?;
            conn.execute(
                r"
                UPDATE agents
                SET status = ?2,
                    current_task_id = ?3,
                    heartbeat_at = CURRENT_TIMESTAMP
                WHERE agent_id = ?1
                ",
                params![agent_id, status.to_string(), current_task_id],
            )?;
            record_agent_heartbeat_in_connection(
                conn,
                &AgentHeartbeatWrite {
                    agent_id,
                    status,
                    current_task_id,
                    related_task_id: current_task_id,
                    source: AgentHeartbeatSource::Heartbeat,
                },
            )?;
            get_agent_in_connection(conn, agent_id)
        })
    }

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
    ) -> StoreResult<Task> {
        self.in_transaction(|conn| {
            create_task_in_connection(conn, title, description, requested_by, project_root)
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
            SELECT task_id, title, description, requested_by, project_root, status,
                   verification_state, priority, severity, owner_agent_id, owner_note,
                   acknowledged_by, acknowledged_at, blocked_reason, verified_by,
                   verified_at, closed_by, closure_summary, closed_at, created_at, updated_at
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

    /// Loads a single agent by id.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent does not exist or the query fails.
    pub fn get_agent(&self, agent_id: &str) -> StoreResult<AgentRegistration> {
        get_agent_in_connection(&self.conn, agent_id)
    }

    /// Updates task lifecycle, verification, and closure metadata.
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
        self.ensure_task_exists(task_id)?;
        self.in_transaction(|conn| {
            let current = get_task_in_connection(conn, task_id)?;
            let from_status = current.status;
            let next_verification = update
                .verification_state
                .unwrap_or(current.verification_state);

            if status == TaskStatus::Blocked && update.blocked_reason.is_none() {
                return Err(StoreError::Validation(
                    "blocked tasks require a blocked reason".to_string(),
                ));
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
                    note: note.as_deref(),
                },
            )?;
            Ok(updated)
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
        action: OperatorActionKind,
        changed_by: &str,
        input: TaskOperatorActionInput<'_>,
    ) -> StoreResult<Task> {
        if let Some(update) = task_operator_triage_update(action, &input)? {
            return self.update_task_triage(task_id, changed_by, update);
        }

        if let Some((status, update)) =
            task_operator_status_update(&self.get_task(task_id)?, action, &input)?
        {
            return self.update_task_status(task_id, status, changed_by, update);
        }

        if let Some(task) = self.apply_task_creation_action(task_id, action, changed_by, &input)? {
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
            | OperatorActionKind::SetTaskPriority
            | OperatorActionKind::SetTaskSeverity
            | OperatorActionKind::BlockTask
            | OperatorActionKind::UnblockTask
            | OperatorActionKind::UpdateTaskNote
            | OperatorActionKind::CreateHandoff
            | OperatorActionKind::PostCouncilMessage
            | OperatorActionKind::AttachEvidence
            | OperatorActionKind::CreateFollowUpTask
            | OperatorActionKind::LinkTaskDependency => unreachable!("handled above"),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn apply_task_creation_action(
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
                    | TaskRelationshipRole::FollowUpChild => {
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
                        note: Some(note.as_str()),
                    },
                )?;
                let inverse_role = match relationship_role {
                    TaskRelationshipRole::Blocks => TaskRelationshipRole::BlockedBy,
                    TaskRelationshipRole::BlockedBy => TaskRelationshipRole::Blocks,
                    TaskRelationshipRole::FollowUpParent
                    | TaskRelationshipRole::FollowUpChild => unreachable!("validated above"),
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
                        note: Some(inverse_note.as_str()),
                    },
                )?;
                touch_task_in_connection(conn, related_task_id)?;
                get_task_in_connection(conn, task_id)
            })?,
            OperatorActionKind::AcknowledgeTask
            | OperatorActionKind::UnacknowledgeTask
            | OperatorActionKind::VerifyTask
            | OperatorActionKind::ReassignTask
            | OperatorActionKind::SetTaskPriority
            | OperatorActionKind::SetTaskSeverity
            | OperatorActionKind::BlockTask
            | OperatorActionKind::UnblockTask
            | OperatorActionKind::UpdateTaskNote
            | OperatorActionKind::AcceptHandoff
            | OperatorActionKind::RejectHandoff
            | OperatorActionKind::CancelHandoff
            | OperatorActionKind::CompleteHandoff
            | OperatorActionKind::FollowUpHandoff
            | OperatorActionKind::ExpireHandoff => return Ok(None),
        };

        Ok(Some(task))
    }

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
                    resolved_by,
                    Some("accepted transfer ownership handoff"),
                )?;
            } else {
                touch_task_in_connection(conn, &handoff.task_id)?;
            }
            let task = get_task_in_connection(conn, &handoff.task_id)?;
            let note = format!(
                "handoff_action=resolve; handoff_id={handoff_id}; status:{}->{}",
                handoff.status, status
            );

            record_task_event_in_connection(
                conn,
                &TaskEventWrite {
                    task_id: &handoff.task_id,
                    event_type: TaskEventType::HandoffUpdated,
                    actor: resolved_by,
                    from_status: Some(task.status),
                    to_status: task.status,
                    verification_state: Some(task.verification_state),
                    owner_agent_id: task.owner_agent_id.as_deref(),
                    note: Some(note.as_str()),
                },
            )?;

            get_handoff_in_connection(conn, handoff_id)
        })
    }

    /// Applies a handoff-scoped operator action using runtime-owned semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if the action is invalid for handoffs or the write
    /// fails.
    pub fn apply_handoff_operator_action(
        &self,
        handoff_id: &str,
        action: OperatorActionKind,
        changed_by: &str,
        input: HandoffOperatorActionInput<'_>,
    ) -> StoreResult<Handoff> {
        match action {
            OperatorActionKind::AcceptHandoff => {
                let _ = input;
                self.resolve_handoff(handoff_id, HandoffStatus::Accepted, changed_by)
            }
            OperatorActionKind::RejectHandoff => {
                let _ = input;
                self.resolve_handoff(handoff_id, HandoffStatus::Rejected, changed_by)
            }
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
                        note: Some(note.as_str()),
                    },
                )?;
                get_handoff_in_connection(conn, handoff_id)
            }),
            OperatorActionKind::AcknowledgeTask
            | OperatorActionKind::UnacknowledgeTask
            | OperatorActionKind::VerifyTask
            | OperatorActionKind::ReassignTask
            | OperatorActionKind::SetTaskPriority
            | OperatorActionKind::SetTaskSeverity
            | OperatorActionKind::BlockTask
            | OperatorActionKind::UnblockTask
            | OperatorActionKind::UpdateTaskNote
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

    /// Lists assignment history globally or for one task.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_task_assignments(&self, task_id: Option<&str>) -> StoreResult<Vec<TaskAssignment>> {
        let mut assignments = Vec::new();
        if let Some(task_id) = task_id {
            let mut stmt = self.conn.prepare(
                r"
                SELECT assignment_id, task_id, assigned_to, assigned_by, reason, assigned_at
                FROM task_assignments
                WHERE task_id = ?1
                ORDER BY rowid
                ",
            )?;
            let rows = stmt.query_map([task_id], map_task_assignment)?;
            for row in rows {
                assignments.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                r"
                SELECT assignment_id, task_id, assigned_to, assigned_by, reason, assigned_at
                FROM task_assignments
                ORDER BY rowid
                ",
            )?;
            let rows = stmt.query_map([], map_task_assignment)?;
            for row in rows {
                assignments.push(row?);
            }
        }
        Ok(assignments)
    }

    /// Appends a council message to a task thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the task or author agent does not exist or if the
    /// write fails.
    pub fn add_council_message(
        &self,
        task_id: &str,
        author_agent_id: &str,
        message_type: CouncilMessageType,
        body: &str,
    ) -> StoreResult<CouncilMessage> {
        self.in_transaction(|conn| {
            add_council_message_in_connection(conn, task_id, author_agent_id, message_type, body)
        })
    }

    /// Lists all council messages for a task in append order.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or if the query fails.
    pub fn list_council_messages(&self, task_id: &str) -> StoreResult<Vec<CouncilMessage>> {
        self.ensure_task_exists(task_id)?;
        let mut stmt = self.conn.prepare(
            r"
            SELECT message_id, task_id, author_agent_id, message_type, body
            FROM council_messages
            WHERE task_id = ?1
            ORDER BY rowid
            ",
        )?;
        let rows = stmt.query_map([task_id], map_council_message)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Attaches an evidence reference to a task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist, a related handoff is
    /// missing, or the write fails.
    pub fn add_evidence(
        &self,
        task_id: &str,
        source_kind: EvidenceSourceKind,
        source_ref: &str,
        label: &str,
        summary: Option<&str>,
        links: EvidenceLinkRefs<'_>,
    ) -> StoreResult<EvidenceRef> {
        self.in_transaction(|conn| {
            add_evidence_in_connection(
                conn,
                task_id,
                source_kind,
                source_ref,
                label,
                summary,
                links,
            )
        })
    }

    /// Lists evidence refs for one task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn list_evidence(&self, task_id: &str) -> StoreResult<Vec<EvidenceRef>> {
        self.ensure_task_exists(task_id)?;
        let mut stmt = self.conn.prepare(
            r"
            SELECT evidence_id, task_id, source_kind, source_ref, label, summary, related_handoff_id,
                   related_session_id, related_memory_query, related_symbol, related_file
            FROM evidence_refs
            WHERE task_id = ?1
            ORDER BY rowid
            ",
        )?;
        let rows = stmt.query_map([task_id], map_evidence)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists all evidence refs across tasks.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_all_evidence(&self) -> StoreResult<Vec<EvidenceRef>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT evidence_id, task_id, source_kind, source_ref, label, summary, related_handoff_id,
                   related_session_id, related_memory_query, related_symbol, related_file
            FROM evidence_refs
            ORDER BY rowid
            ",
        )?;
        let rows = stmt.query_map([], map_evidence)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists heartbeat events, optionally filtered by agent or task.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_agent_heartbeats(
        &self,
        agent_id: Option<&str>,
        task_id: Option<&str>,
        limit: usize,
    ) -> StoreResult<Vec<AgentHeartbeatEvent>> {
        let limit = limit.max(1);
        let limit_i64 = i64::try_from(limit).map_err(|_| {
            StoreError::Validation("heartbeat limit exceeds supported range".to_string())
        })?;

        let mut heartbeats = Vec::new();
        match (agent_id, task_id) {
            (Some(agent_id), Some(task_id)) => {
                let mut stmt = self.conn.prepare(
                    r"
                    SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
                    FROM agent_heartbeat_events
                    WHERE agent_id = ?1 AND (current_task_id = ?2 OR related_task_id = ?2)
                    ORDER BY rowid DESC
                    LIMIT ?3
                    ",
                )?;
                let rows =
                    stmt.query_map(params![agent_id, task_id, limit_i64], map_agent_heartbeat)?;
                for row in rows {
                    heartbeats.push(row?);
                }
            }
            (Some(agent_id), None) => {
                let mut stmt = self.conn.prepare(
                    r"
                    SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
                    FROM agent_heartbeat_events
                    WHERE agent_id = ?1
                    ORDER BY rowid DESC
                    LIMIT ?2
                    ",
                )?;
                let rows = stmt.query_map(params![agent_id, limit_i64], map_agent_heartbeat)?;
                for row in rows {
                    heartbeats.push(row?);
                }
            }
            (None, Some(task_id)) => {
                let mut stmt = self.conn.prepare(
                    r"
                    SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
                    FROM agent_heartbeat_events
                    WHERE current_task_id = ?1 OR related_task_id = ?1
                    ORDER BY rowid DESC
                    LIMIT ?2
                    ",
                )?;
                let rows = stmt.query_map(params![task_id, limit_i64], map_agent_heartbeat)?;
                for row in rows {
                    heartbeats.push(row?);
                }
            }
            (None, None) => {
                let mut stmt = self.conn.prepare(
                    r"
                    SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
                    FROM agent_heartbeat_events
                    ORDER BY rowid DESC
                    LIMIT ?1
                    ",
                )?;
                let rows = stmt.query_map(params![limit_i64], map_agent_heartbeat)?;
                for row in rows {
                    heartbeats.push(row?);
                }
            }
        }

        Ok(heartbeats)
    }

    /// Lists all heartbeat events without pre-filter truncation.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_all_agent_heartbeats(&self) -> StoreResult<Vec<AgentHeartbeatEvent>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
            FROM agent_heartbeat_events
            ORDER BY rowid DESC
            ",
        )?;
        let rows = stmt.query_map([], map_agent_heartbeat)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists heartbeat history relevant to a task, including stop/idle events.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn list_task_heartbeats(
        &self,
        task_id: &str,
        limit: usize,
    ) -> StoreResult<Vec<AgentHeartbeatEvent>> {
        let task = self.get_task(task_id)?;
        let limit_i64 = i64::try_from(limit.max(1)).map_err(|_| {
            StoreError::Validation("heartbeat limit exceeds supported range".to_string())
        })?;

        let mut stmt = self.conn.prepare(
            r"
            WITH related_agents AS (
                SELECT owner_agent_id AS agent_id
                FROM task_events
                WHERE task_id = ?1 AND owner_agent_id IS NOT NULL
                UNION
                SELECT from_agent_id AS agent_id
                FROM handoffs
                WHERE task_id = ?1
                UNION
                SELECT to_agent_id AS agent_id
                FROM handoffs
                WHERE task_id = ?1
                UNION
                SELECT owner_agent_id AS agent_id
                FROM tasks
                WHERE task_id = ?1 AND owner_agent_id IS NOT NULL
            )
            SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
            FROM agent_heartbeat_events
            WHERE agent_id IN (SELECT agent_id FROM related_agents)
              AND created_at >= ?2
              AND (current_task_id = ?1 OR related_task_id = ?1)
            ORDER BY rowid DESC
            LIMIT ?3
            ",
        )?;
        let rows = stmt.query_map(
            params![task_id, task.created_at, limit_i64],
            map_agent_heartbeat,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists timeline events for one task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn list_task_events(&self, task_id: &str) -> StoreResult<Vec<TaskEvent>> {
        self.ensure_task_exists(task_id)?;
        let mut stmt = self.conn.prepare(
            r"
            SELECT event_id, task_id, event_type, actor, from_status, to_status,
                   verification_state, owner_agent_id, note, created_at
            FROM task_events
            WHERE task_id = ?1
            ORDER BY rowid
            ",
        )?;
        let rows = stmt.query_map([task_id], map_task_event)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists task relationships globally or for one task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn list_task_relationships(
        &self,
        task_id: Option<&str>,
    ) -> StoreResult<Vec<TaskRelationship>> {
        let mut relationships = Vec::new();
        if let Some(task_id) = task_id {
            self.ensure_task_exists(task_id)?;
            let mut stmt = self.conn.prepare(
                r"
                SELECT relationship_id, source_task_id, target_task_id, kind, created_by, created_at, updated_at
                FROM task_relationships
                WHERE source_task_id = ?1 OR target_task_id = ?1
                ORDER BY rowid
                ",
            )?;
            let rows = stmt.query_map([task_id], map_task_relationship)?;
            for row in rows {
                relationships.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                r"
                SELECT relationship_id, source_task_id, target_task_id, kind, created_by, created_at, updated_at
                FROM task_relationships
                ORDER BY rowid
                ",
            )?;
            let rows = stmt.query_map([], map_task_relationship)?;
            for row in rows {
                relationships.push(row?);
            }
        }
        Ok(relationships)
    }

    /// Loads directional related-task summaries for one task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn list_related_tasks(&self, task_id: &str) -> StoreResult<Vec<RelatedTask>> {
        self.ensure_task_exists(task_id)?;
        let relationships = self.list_task_relationships(Some(task_id))?;
        relationships
            .into_iter()
            .map(|relationship| {
                let (related_task_id, relationship_role) = if relationship.source_task_id == task_id {
                    let role = match relationship.kind {
                        TaskRelationshipKind::FollowUp => TaskRelationshipRole::FollowUpChild,
                        TaskRelationshipKind::Blocks => TaskRelationshipRole::Blocks,
                    };
                    (relationship.target_task_id.clone(), role)
                } else {
                    let role = match relationship.kind {
                        TaskRelationshipKind::FollowUp => TaskRelationshipRole::FollowUpParent,
                        TaskRelationshipKind::Blocks => TaskRelationshipRole::BlockedBy,
                    };
                    (relationship.source_task_id.clone(), role)
                };
                let related_task = self.get_task(&related_task_id)?;
                Ok(RelatedTask {
                    relationship_id: relationship.relationship_id,
                    relationship_kind: relationship.kind,
                    relationship_role,
                    related_task_id: related_task.task_id,
                    title: related_task.title,
                    status: related_task.status,
                    verification_state: related_task.verification_state,
                    priority: related_task.priority,
                    severity: related_task.severity,
                    owner_agent_id: related_task.owner_agent_id,
                    blocked_reason: related_task.blocked_reason,
                    created_at: related_task.created_at,
                    updated_at: related_task.updated_at,
                })
            })
            .collect()
    }

    fn ensure_task_exists(&self, task_id: &str) -> StoreResult<()> {
        let exists = self
            .conn
            .query_row("SELECT 1 FROM tasks WHERE task_id = ?1", [task_id], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?;
        exists.ok_or(StoreError::NotFound("task"))?;
        Ok(())
    }

    fn ensure_agent_exists(&self, agent_id: &str) -> StoreResult<()> {
        let exists = self
            .conn
            .query_row(
                "SELECT 1 FROM agents WHERE agent_id = ?1",
                [agent_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        exists.ok_or(StoreError::NotFound("agent"))?;
        Ok(())
    }

    fn in_transaction<T>(
        &self,
        operation: impl FnOnce(&Connection) -> StoreResult<T>,
    ) -> StoreResult<T> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        match operation(&self.conn) {
            Ok(value) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(value)
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EvidenceNavigation<'a> {
    session_id: Option<&'a str>,
    memory_query: Option<&'a str>,
    symbol: Option<&'a str>,
    file: Option<&'a str>,
}

fn create_task_in_connection(
    conn: &Connection,
    title: &str,
    description: Option<&str>,
    requested_by: &str,
    project_root: &str,
) -> StoreResult<Task> {
    let task = Task {
        task_id: Ulid::new().to_string(),
        title: title.to_string(),
        description: description.map(ToOwned::to_owned),
        requested_by: requested_by.to_string(),
        project_root: project_root.to_string(),
        status: TaskStatus::Open,
        verification_state: VerificationState::Unknown,
        priority: TaskPriority::Medium,
        severity: TaskSeverity::None,
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
        created_at: String::new(),
        updated_at: String::new(),
    };
    conn.execute(
        r"
        INSERT INTO tasks (
            task_id, title, description, requested_by, project_root, status,
            verification_state, priority, severity, owner_agent_id, owner_note,
            acknowledged_by, acknowledged_at, blocked_reason, verified_by, verified_at,
            closed_by, closure_summary, closed_at, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ",
        params![
            task.task_id,
            task.title,
            task.description,
            task.requested_by,
            task.project_root,
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
            note: description,
        },
    )?;
    get_task_in_connection(conn, &task.task_id)
}

#[allow(clippy::too_many_arguments)]
fn create_handoff_in_connection(
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

fn add_council_message_in_connection(
    conn: &Connection,
    task_id: &str,
    author_agent_id: &str,
    message_type: CouncilMessageType,
    body: &str,
) -> StoreResult<CouncilMessage> {
    get_task_in_connection(conn, task_id)?;
    get_agent_in_connection(conn, author_agent_id)?;
    if body.trim().is_empty() {
        return Err(StoreError::Validation(
            "council messages require a non-empty body".to_string(),
        ));
    }

    let message = CouncilMessage {
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

fn add_evidence_in_connection(
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
            evidence_id, task_id, source_kind, source_ref, label, summary, related_handoff_id,
            related_session_id, related_memory_query, related_symbol, related_file
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ",
        params![
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

fn create_task_relationship_in_connection(
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
    touch_task_in_connection(conn, source_task_id)?;
    touch_task_in_connection(conn, target_task_id)?;
    get_task_relationship_in_connection(conn, &relationship.relationship_id)
}

fn migrate_schema(conn: &Connection) -> StoreResult<()> {
    ensure_column(conn, "tasks", "priority", "TEXT NULL")?;
    ensure_column(conn, "tasks", "severity", "TEXT NULL")?;
    ensure_column(conn, "tasks", "owner_note", "TEXT NULL")?;
    ensure_column(conn, "tasks", "acknowledged_by", "TEXT NULL")?;
    ensure_column(conn, "tasks", "acknowledged_at", "TEXT NULL")?;
    ensure_column(conn, "tasks", "created_at", "TEXT NULL")?;
    ensure_column(conn, "tasks", "updated_at", "TEXT NULL")?;
    conn.execute(
        r"
        UPDATE tasks
        SET priority = COALESCE(priority, 'medium'),
            severity = COALESCE(severity, 'none'),
            created_at = COALESCE(
                created_at,
                (SELECT MIN(created_at) FROM task_events WHERE task_events.task_id = tasks.task_id),
                CURRENT_TIMESTAMP
            ),
            updated_at = COALESCE(
                updated_at,
                (SELECT MAX(created_at) FROM task_events WHERE task_events.task_id = tasks.task_id),
                closed_at,
                verified_at,
                created_at,
                CURRENT_TIMESTAMP
            )
        ",
        [],
    )?;

    ensure_column(conn, "handoffs", "due_at", "TEXT NULL")?;
    ensure_column(conn, "handoffs", "expires_at", "TEXT NULL")?;
    ensure_column(conn, "handoffs", "created_at", "TEXT NULL")?;
    ensure_column(conn, "handoffs", "updated_at", "TEXT NULL")?;
    ensure_column(conn, "handoffs", "resolved_at", "TEXT NULL")?;
    conn.execute(
        r"
        UPDATE handoffs
        SET created_at = COALESCE(
                created_at,
                (SELECT created_at FROM tasks WHERE tasks.task_id = handoffs.task_id),
                CURRENT_TIMESTAMP
            ),
            updated_at = COALESCE(
                updated_at,
                resolved_at,
                (SELECT updated_at FROM tasks WHERE tasks.task_id = handoffs.task_id),
                created_at,
                CURRENT_TIMESTAMP
            )
        ",
        [],
    )?;

    ensure_column(conn, "evidence_refs", "related_session_id", "TEXT NULL")?;
    ensure_column(conn, "evidence_refs", "related_memory_query", "TEXT NULL")?;
    ensure_column(conn, "evidence_refs", "related_symbol", "TEXT NULL")?;
    ensure_column(conn, "evidence_refs", "related_file", "TEXT NULL")?;
    ensure_column(
        conn,
        "agent_heartbeat_events",
        "related_task_id",
        "TEXT NULL",
    )?;

    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> StoreResult<()> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&pragma)?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;

    for existing in columns {
        if existing? == column {
            return Ok(());
        }
    }

    let alter = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
    conn.execute(&alter, [])?;
    Ok(())
}

fn validate_handoff_timing(timing: HandoffTiming<'_>) -> StoreResult<()> {
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

fn parse_rfc3339_timestamp(raw: &str) -> StoreResult<OffsetDateTime> {
    OffsetDateTime::parse(raw, &Rfc3339)
        .map_err(|_| StoreError::Validation(format!("invalid RFC3339 timestamp: {raw}")))
}

fn handoff_is_expired(handoff: &Handoff) -> StoreResult<bool> {
    let Some(expires_at) = handoff.expires_at.as_deref() else {
        return Ok(false);
    };
    Ok(parse_rfc3339_timestamp(expires_at)? <= OffsetDateTime::now_utc())
}

fn normalize_evidence_navigation<'a>(
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
        | EvidenceSourceKind::MyceliumCommand
        | EvidenceSourceKind::MyceliumExplain => EvidenceNavigation {
            session_id,
            memory_query,
            symbol,
            file,
        },
    }
}

#[allow(clippy::too_many_lines)]
fn assign_task_in_connection(
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

fn sync_owner_for_task_status(
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

fn task_operator_triage_update<'a>(
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
        OperatorActionKind::VerifyTask
        | OperatorActionKind::ReassignTask
        | OperatorActionKind::BlockTask
        | OperatorActionKind::UnblockTask
        | OperatorActionKind::CreateHandoff
            | OperatorActionKind::PostCouncilMessage
            | OperatorActionKind::AttachEvidence
            | OperatorActionKind::CreateFollowUpTask
            | OperatorActionKind::LinkTaskDependency
            | OperatorActionKind::AcceptHandoff
        | OperatorActionKind::RejectHandoff
        | OperatorActionKind::CancelHandoff
        | OperatorActionKind::CompleteHandoff
        | OperatorActionKind::FollowUpHandoff
        | OperatorActionKind::ExpireHandoff => return Ok(None),
    };

    Ok(Some(update))
}

fn task_operator_status_update<'a>(
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
            if verification_state == VerificationState::Passed
                && input
                    .closure_summary
                    .is_none_or(|summary| summary.trim().is_empty())
            {
                return Err(StoreError::Validation(
                    "verify_task passed reviews require a closure summary".to_string(),
                ));
            }

            let status = match verification_state {
                VerificationState::Passed => TaskStatus::Completed,
                VerificationState::Pending | VerificationState::Failed => {
                    TaskStatus::ReviewRequired
                }
                VerificationState::Unknown => unreachable!("validated above"),
            };

            (
                status,
                TaskStatusUpdate {
                    verification_state: Some(verification_state),
                    closure_summary: if status == TaskStatus::Completed {
                        input.closure_summary
                    } else {
                        None
                    },
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
        OperatorActionKind::AcknowledgeTask
        | OperatorActionKind::UnacknowledgeTask
        | OperatorActionKind::ReassignTask
        | OperatorActionKind::SetTaskPriority
        | OperatorActionKind::SetTaskSeverity
        | OperatorActionKind::UpdateTaskNote
        | OperatorActionKind::CreateHandoff
        | OperatorActionKind::PostCouncilMessage
        | OperatorActionKind::AttachEvidence
        | OperatorActionKind::CreateFollowUpTask
        | OperatorActionKind::LinkTaskDependency
        | OperatorActionKind::AcceptHandoff
        | OperatorActionKind::RejectHandoff
        | OperatorActionKind::CancelHandoff
        | OperatorActionKind::CompleteHandoff
        | OperatorActionKind::FollowUpHandoff
        | OperatorActionKind::ExpireHandoff => return Ok(None),
    };

    Ok(Some(update))
}

fn get_task_in_connection(conn: &Connection, task_id: &str) -> StoreResult<Task> {
    conn.query_row(
        r"
        SELECT task_id, title, description, requested_by, project_root, status,
               verification_state, priority, severity, owner_agent_id, owner_note,
               acknowledged_by, acknowledged_at, blocked_reason, verified_by,
               verified_at, closed_by, closure_summary, closed_at, created_at, updated_at
        FROM tasks
        WHERE task_id = ?1
        ",
        [task_id],
        map_task,
    )
    .optional()?
    .ok_or(StoreError::NotFound("task"))
}

fn get_task_relationship_in_connection(
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

fn validate_agent_task_link(
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

fn validate_agent_registration(conn: &Connection, agent: &AgentRegistration) -> StoreResult<()> {
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

fn touch_task_in_connection(conn: &Connection, task_id: &str) -> StoreResult<()> {
    conn.execute(
        "UPDATE tasks SET updated_at = CURRENT_TIMESTAMP WHERE task_id = ?1",
        [task_id],
    )?;
    Ok(())
}

fn record_task_event_in_connection(
    conn: &Connection,
    event: &TaskEventWrite<'_>,
) -> StoreResult<()> {
    conn.execute(
        r"
        INSERT INTO task_events (
            event_id, task_id, event_type, actor, from_status, to_status,
            verification_state, owner_agent_id, note
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
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
            event.note,
        ],
    )?;
    Ok(())
}

fn record_agent_heartbeat_in_connection(
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

fn get_handoff_in_connection(conn: &Connection, handoff_id: &str) -> StoreResult<Handoff> {
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

fn get_agent_in_connection(conn: &Connection, agent_id: &str) -> StoreResult<AgentRegistration> {
    conn.query_row(
        r"
        SELECT agent_id, host_id, host_type, host_instance, model,
               project_root, worktree_id, status, current_task_id, heartbeat_at
        FROM agents
        WHERE agent_id = ?1
        ",
        [agent_id],
        map_agent,
    )
    .optional()?
    .ok_or(StoreError::NotFound("agent"))
}

fn map_agent(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRegistration> {
    Ok(AgentRegistration {
        agent_id: row.get(0)?,
        host_id: row.get(1)?,
        host_type: row.get(2)?,
        host_instance: row.get(3)?,
        model: row.get(4)?,
        project_root: row.get(5)?,
        worktree_id: row.get(6)?,
        status: parse_enum_column(row, 7)?,
        current_task_id: row.get(8)?,
        heartbeat_at: row.get(9)?,
    })
}

fn map_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        task_id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        requested_by: row.get(3)?,
        project_root: row.get(4)?,
        status: parse_enum_column(row, 5)?,
        verification_state: parse_enum_column(row, 6)?,
        priority: parse_enum_column(row, 7)?,
        severity: parse_enum_column(row, 8)?,
        owner_agent_id: row.get(9)?,
        owner_note: row.get(10)?,
        acknowledged_by: row.get(11)?,
        acknowledged_at: row.get(12)?,
        blocked_reason: row.get(13)?,
        verified_by: row.get(14)?,
        verified_at: row.get(15)?,
        closed_by: row.get(16)?,
        closure_summary: row.get(17)?,
        closed_at: row.get(18)?,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
    })
}

fn map_handoff(row: &rusqlite::Row<'_>) -> rusqlite::Result<Handoff> {
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

fn map_task_assignment(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskAssignment> {
    Ok(TaskAssignment {
        assignment_id: row.get(0)?,
        task_id: row.get(1)?,
        assigned_to: row.get(2)?,
        assigned_by: row.get(3)?,
        reason: row.get(4)?,
        assigned_at: row.get(5)?,
    })
}

fn map_council_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<CouncilMessage> {
    Ok(CouncilMessage {
        message_id: row.get(0)?,
        task_id: row.get(1)?,
        author_agent_id: row.get(2)?,
        message_type: parse_enum_column(row, 3)?,
        body: row.get(4)?,
    })
}

fn map_evidence(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvidenceRef> {
    Ok(EvidenceRef {
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

fn map_task_relationship(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRelationship> {
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

fn map_task_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskEvent> {
    Ok(TaskEvent {
        event_id: row.get(0)?,
        task_id: row.get(1)?,
        event_type: parse_enum_column(row, 2)?,
        actor: row.get(3)?,
        from_status: parse_optional_enum_column(row, 4)?,
        to_status: parse_enum_column(row, 5)?,
        verification_state: parse_optional_enum_column(row, 6)?,
        owner_agent_id: row.get(7)?,
        note: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn map_agent_heartbeat(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentHeartbeatEvent> {
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

fn parse_enum_column<T>(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    <T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
{
    let value: String = row.get(index)?;
    parse_enum_value::<T>(&value, index)
}

fn parse_optional_enum_column<T>(
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

fn parse_enum_value<T>(value: &str, index: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    <T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
{
    T::from_str(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
    })
}
