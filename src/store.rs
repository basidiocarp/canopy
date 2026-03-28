use crate::models::{
    AgentRegistration, AgentStatus, CouncilMessage, CouncilMessageType, EvidenceRef,
    EvidenceSourceKind, Handoff, HandoffStatus, HandoffType, Task, TaskStatus,
};
use rusqlite::{Connection, OptionalExtension, params, types::Type};
use std::fs;
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;
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
        conn.execute_batch(
            r"
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
                owner_agent_id TEXT NULL REFERENCES agents(agent_id)
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
                status TEXT NOT NULL
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
                related_handoff_id TEXT NULL REFERENCES handoffs(handoff_id)
            );
            ",
        )?;

        Ok(Self { conn })
    }

    /// Registers or refreshes an agent entry in the local registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying database write fails.
    pub fn register_agent(&self, agent: &AgentRegistration) -> StoreResult<AgentRegistration> {
        self.conn.execute(
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
        self.get_agent(&agent.agent_id)
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
        self.conn.execute(
            r"
            UPDATE agents
            SET status = ?2,
                current_task_id = ?3,
                heartbeat_at = CURRENT_TIMESTAMP
            WHERE agent_id = ?1
            ",
            params![agent_id, status.to_string(), current_task_id],
        )?;
        self.get_agent(agent_id)
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
            owner_agent_id: None,
        };
        self.conn.execute(
            r"
            INSERT INTO tasks (
                task_id, title, description, requested_by, project_root, status, owner_agent_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                task.task_id,
                task.title,
                task.description,
                task.requested_by,
                task.project_root,
                task.status.to_string(),
                task.owner_agent_id,
            ],
        )?;
        Ok(task)
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
            SELECT task_id, title, description, requested_by, project_root, status, owner_agent_id
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
        self.conn
            .query_row(
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
    pub fn create_handoff(
        &self,
        task_id: &str,
        from_agent_id: &str,
        to_agent_id: &str,
        handoff_type: HandoffType,
        summary: &str,
        requested_action: Option<&str>,
    ) -> StoreResult<Handoff> {
        self.ensure_task_exists(task_id)?;
        self.ensure_agent_exists(from_agent_id)?;
        self.ensure_agent_exists(to_agent_id)?;

        let handoff = Handoff {
            handoff_id: Ulid::new().to_string(),
            task_id: task_id.to_string(),
            from_agent_id: from_agent_id.to_string(),
            to_agent_id: to_agent_id.to_string(),
            handoff_type,
            summary: summary.to_string(),
            requested_action: requested_action.map(ToOwned::to_owned),
            status: HandoffStatus::Open,
        };
        self.conn.execute(
            r"
            INSERT INTO handoffs (
                handoff_id, task_id, from_agent_id, to_agent_id, handoff_type,
                summary, requested_action, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ",
            params![
                handoff.handoff_id,
                handoff.task_id,
                handoff.from_agent_id,
                handoff.to_agent_id,
                handoff.handoff_type.to_string(),
                handoff.summary,
                handoff.requested_action,
                handoff.status.to_string(),
            ],
        )?;
        Ok(handoff)
    }

    /// Resolves an existing handoff with a terminal or accepted state.
    ///
    /// # Errors
    ///
    /// Returns an error if the handoff does not exist, the requested status is
    /// unsupported, or the update fails.
    pub fn resolve_handoff(&self, handoff_id: &str, status: HandoffStatus) -> StoreResult<Handoff> {
        self.in_transaction(|conn| {
            let handoff = get_handoff_in_connection(conn, handoff_id)?;
            let updated = conn.execute(
                "UPDATE handoffs SET status = ?2 WHERE handoff_id = ?1",
                params![handoff_id, status.to_string()],
            )?;
            if updated == 0 {
                return Err(StoreError::NotFound("handoff"));
            }

            if status == HandoffStatus::Accepted {
                assign_task_in_connection(
                    conn,
                    &handoff.task_id,
                    &handoff.to_agent_id,
                    &handoff.from_agent_id,
                    Some("accepted handoff"),
                )?;
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
                       summary, requested_action, status
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
                       summary, requested_action, status
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
        related_handoff_id: Option<&str>,
    ) -> StoreResult<EvidenceRef> {
        self.ensure_task_exists(task_id)?;
        if let Some(handoff_id) = related_handoff_id {
            let _ = self.list_handoffs(Some(task_id))?;
            self.get_handoff(handoff_id)?;
        }

        let evidence = EvidenceRef {
            evidence_id: Ulid::new().to_string(),
            task_id: task_id.to_string(),
            source_kind,
            source_ref: source_ref.to_string(),
            label: label.to_string(),
            summary: summary.map(ToOwned::to_owned),
            related_handoff_id: related_handoff_id.map(ToOwned::to_owned),
        };
        self.conn.execute(
            r"
            INSERT INTO evidence_refs (
                evidence_id, task_id, source_kind, source_ref, label, summary, related_handoff_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                evidence.evidence_id,
                evidence.task_id,
                evidence.source_kind.to_string(),
                evidence.source_ref,
                evidence.label,
                evidence.summary,
                evidence.related_handoff_id
            ],
        )?;
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
            SELECT evidence_id, task_id, source_kind, source_ref, label, summary, related_handoff_id
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
            SELECT evidence_id, task_id, source_kind, source_ref, label, summary, related_handoff_id
            FROM evidence_refs
            ORDER BY rowid
            ",
        )?;
        let rows = stmt.query_map([], map_evidence)?;
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
}

fn assign_task_in_connection(
    conn: &Connection,
    task_id: &str,
    assigned_to: &str,
    assigned_by: &str,
    reason: Option<&str>,
) -> StoreResult<()> {
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
        SET owner_agent_id = ?2, status = 'assigned'
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

    if let Some(previous_owner) = previous_owner.filter(|owner| owner != assigned_to) {
        conn.execute(
            r"
            UPDATE agents
            SET current_task_id = NULL, status = 'idle', heartbeat_at = CURRENT_TIMESTAMP
            WHERE agent_id = ?1 AND current_task_id = ?2
            ",
            params![previous_owner, task_id],
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

    Ok(())
}

fn get_task_in_connection(conn: &Connection, task_id: &str) -> StoreResult<Task> {
    conn.query_row(
        r"
        SELECT task_id, title, description, requested_by, project_root, status, owner_agent_id
        FROM tasks
        WHERE task_id = ?1
        ",
        [task_id],
        map_task,
    )
    .optional()?
    .ok_or(StoreError::NotFound("task"))
}

fn get_handoff_in_connection(conn: &Connection, handoff_id: &str) -> StoreResult<Handoff> {
    conn.query_row(
        r"
        SELECT handoff_id, task_id, from_agent_id, to_agent_id, handoff_type,
               summary, requested_action, status
        FROM handoffs
        WHERE handoff_id = ?1
        ",
        [handoff_id],
        map_handoff,
    )
    .optional()?
    .ok_or(StoreError::NotFound("handoff"))
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
        owner_agent_id: row.get(6)?,
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
        status: parse_enum_column(row, 7)?,
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
    })
}

fn parse_enum_column<T>(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    <T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
{
    let value: String = row.get(index)?;
    T::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
    })
}
