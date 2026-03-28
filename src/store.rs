use crate::models::{
    AgentHeartbeatEvent, AgentHeartbeatSource, AgentRegistration, AgentStatus, CouncilMessage,
    CouncilMessageType, EvidenceRef, EvidenceSourceKind, Handoff, HandoffStatus, HandoffType, Task,
    TaskEvent, TaskEventType, TaskPriority, TaskSeverity, TaskStatus, VerificationState,
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
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HandoffTiming<'a> {
    pub due_at: Option<&'a str>,
    pub expires_at: Option<&'a str>,
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
        self.conn.execute(
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
        self.record_task_event(&TaskEventWrite {
            task_id: &task.task_id,
            event_type: TaskEventType::Created,
            actor: requested_by,
            from_status: None,
            to_status: TaskStatus::Open,
            verification_state: Some(VerificationState::Unknown),
            owner_agent_id: None,
            note: description,
        })?;
        self.get_task(&task.task_id)
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
        verification_state: Option<VerificationState>,
        blocked_reason: Option<&str>,
        closure_summary: Option<&str>,
    ) -> StoreResult<Task> {
        self.ensure_task_exists(task_id)?;
        self.in_transaction(|conn| {
            let current = get_task_in_connection(conn, task_id)?;
            let from_status = current.status;
            let next_verification = verification_state.unwrap_or(current.verification_state);

            if status == TaskStatus::Blocked && blocked_reason.is_none() {
                return Err(StoreError::Validation(
                    "blocked tasks require a blocked reason".to_string(),
                ));
            }

            let (verified_by, verified_at) = if verification_state.is_some() {
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
                        blocked_reason
                    } else {
                        None
                    },
                    verified_by,
                    verified_at,
                    if is_terminal { Some(changed_by) } else { None },
                    if is_terminal { closure_summary } else { None },
                    is_terminal,
                ],
            )?;

            sync_owner_for_task_status(conn, task_id, status)?;
            let updated = get_task_in_connection(conn, task_id)?;
            let note = match status {
                TaskStatus::Blocked => blocked_reason,
                TaskStatus::Completed | TaskStatus::Closed | TaskStatus::Cancelled => {
                    closure_summary
                }
                _ => None,
            };
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
                    note,
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
            if update.priority.is_some() {
                notes.push(format!("priority={}", updated.priority));
            }
            if update.severity.is_some() {
                notes.push(format!("severity={}", updated.severity));
            }
            if let Some(acknowledged) = update.acknowledged {
                notes.push(format!("acknowledged={acknowledged}"));
            }
            if update.owner_note.is_some() {
                notes.push("owner_note_updated=true".to_string());
            }
            if update.clear_owner_note {
                notes.push("owner_note_cleared=true".to_string());
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
        self.ensure_task_exists(task_id)?;
        self.ensure_agent_exists(from_agent_id)?;
        self.ensure_agent_exists(to_agent_id)?;
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
        self.conn.execute(
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
        touch_task_in_connection(&self.conn, task_id)?;
        self.get_handoff(&handoff.handoff_id)
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

            get_handoff_in_connection(conn, handoff_id)
        })
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
        self.ensure_task_exists(task_id)?;
        self.ensure_agent_exists(author_agent_id)?;

        let message = CouncilMessage {
            message_id: Ulid::new().to_string(),
            task_id: task_id.to_string(),
            author_agent_id: author_agent_id.to_string(),
            message_type,
            body: body.to_string(),
        };
        self.conn.execute(
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
        touch_task_in_connection(&self.conn, task_id)?;
        Ok(message)
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
        self.ensure_task_exists(task_id)?;
        if let Some(handoff_id) = links.related_handoff_id {
            let handoff = self.get_handoff(handoff_id)?;
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
        self.conn.execute(
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
        touch_task_in_connection(&self.conn, task_id)?;
        Ok(evidence)
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
              AND created_at <= ?3
              AND (current_task_id = ?1 OR related_task_id = ?1)
            ORDER BY rowid DESC
            LIMIT ?4
            ",
        )?;
        let rows = stmt.query_map(
            params![task_id, task.created_at, task.updated_at, limit_i64],
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

    fn record_task_event(&self, event: &TaskEventWrite<'_>) -> StoreResult<()> {
        record_task_event_in_connection(&self.conn, event)
    }
}

#[derive(Debug, Clone, Copy)]
struct EvidenceNavigation<'a> {
    session_id: Option<&'a str>,
    memory_query: Option<&'a str>,
    symbol: Option<&'a str>,
    file: Option<&'a str>,
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
            note: reason,
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
