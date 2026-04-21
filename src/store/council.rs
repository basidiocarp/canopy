use super::helpers::{
    add_council_message_in_connection, map_council_message, parse_enum_value,
    sync_task_workflow_in_connection,
};
use super::{Store, StoreError, StoreResult};
use crate::models::{
    CouncilMessage, CouncilMessageType, CouncilParticipant, CouncilParticipantRole,
    CouncilParticipantStatus, CouncilSession, CouncilSessionState, CouncilSessionTimelineEntry,
    CouncilSessionTimelineKind, Task,
};
use rusqlite::{Connection, OptionalExtension, params};
use ulid::Ulid;

const DEFAULT_TIMELINE_REF_TEMPLATE: &str = "task:{task_id}:council_messages";

#[derive(Debug)]
struct StoredCouncilSession {
    council_session_id: String,
    task_id: String,
    worktree_id: Option<String>,
    participants: Vec<CouncilParticipant>,
    state: CouncilSessionState,
    session_summary: Option<String>,
    transcript_ref: Option<String>,
    opened_at: String,
    updated_at: Option<String>,
    closed_at: Option<String>,
}

fn default_participants() -> Vec<CouncilParticipant> {
    vec![
        CouncilParticipant {
            role: CouncilParticipantRole::Reviewer,
            agent_id: None,
            status: Some(CouncilParticipantStatus::Summoned),
        },
        CouncilParticipant {
            role: CouncilParticipantRole::Architect,
            agent_id: None,
            status: Some(CouncilParticipantStatus::Summoned),
        },
    ]
}

fn participants_json(participants: &[CouncilParticipant]) -> StoreResult<String> {
    serde_json::to_string(participants).map_err(|error| StoreError::Validation(error.to_string()))
}

fn participant_worktree_id_in_connection(
    conn: &Connection,
    task: &Task,
) -> StoreResult<Option<String>> {
    let Some(owner_agent_id) = task.owner_agent_id.as_deref() else {
        return Ok(None);
    };
    let worktree_id = conn
        .query_row(
            "SELECT worktree_id FROM agents WHERE agent_id = ?1",
            [owner_agent_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(worktree_id)
}

fn timeline_kind(message_type: CouncilMessageType) -> CouncilSessionTimelineKind {
    match message_type {
        CouncilMessageType::Decision => CouncilSessionTimelineKind::Decision,
        CouncilMessageType::Evidence => CouncilSessionTimelineKind::Output,
        CouncilMessageType::Proposal
        | CouncilMessageType::Objection
        | CouncilMessageType::Handoff
        | CouncilMessageType::Status => CouncilSessionTimelineKind::Response,
    }
}

fn timeline_title(message_type: CouncilMessageType) -> String {
    match message_type {
        CouncilMessageType::Decision => "Decision recorded".to_string(),
        CouncilMessageType::Evidence => "Evidence attached".to_string(),
        CouncilMessageType::Proposal => "Proposal submitted".to_string(),
        CouncilMessageType::Objection => "Objection raised".to_string(),
        CouncilMessageType::Handoff => "Council handoff".to_string(),
        CouncilMessageType::Status => "Council update".to_string(),
    }
}

fn build_timeline(
    session: &StoredCouncilSession,
    messages: &[CouncilMessage],
) -> Vec<CouncilSessionTimelineEntry> {
    let mut timeline = vec![CouncilSessionTimelineEntry {
        actor_agent_id: None,
        body: "Summoned fixed reviewer and architect roles for this task.".to_string(),
        created_at: Some(session.opened_at.clone()),
        kind: CouncilSessionTimelineKind::Summon,
        title: Some("Council summoned".to_string()),
    }];

    timeline.extend(messages.iter().map(|message| CouncilSessionTimelineEntry {
        actor_agent_id: Some(message.author_agent_id.clone()),
        body: message.body.clone(),
        created_at: message.created_at.clone(),
        kind: timeline_kind(message.message_type),
        title: Some(timeline_title(message.message_type)),
    }));

    if session.state == CouncilSessionState::Closed {
        timeline.push(CouncilSessionTimelineEntry {
            actor_agent_id: None,
            body: "Council session closed.".to_string(),
            created_at: session.closed_at.clone(),
            kind: CouncilSessionTimelineKind::Closure,
            title: Some("Council closed".to_string()),
        });
    }

    timeline
}

fn build_session(raw: StoredCouncilSession, messages: &[CouncilMessage]) -> CouncilSession {
    let timeline = build_timeline(&raw, messages);
    CouncilSession {
        council_session_id: raw.council_session_id,
        task_id: raw.task_id,
        worktree_id: raw.worktree_id,
        participants: raw.participants,
        // Return None when no explicit summary has been set. Callers use
        // Option<String> and can display a placeholder at the presentation layer.
        // Synthesising a non-None value here made it impossible to distinguish
        // "no summary yet" from an actual decision summary.
        session_summary: raw.session_summary,
        state: raw.state,
        timeline,
        transcript_ref: raw.transcript_ref,
        created_at: raw.opened_at,
        updated_at: raw
            .updated_at
            .or(raw.closed_at)
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
    }
}

pub(crate) fn summon_task_council_in_connection(
    conn: &Connection,
    task: &Task,
    transcript_ref: Option<&str>,
) -> StoreResult<()> {
    let existing = conn
        .query_row(
            "SELECT council_session_id FROM council_sessions WHERE task_id = ?1",
            [task.task_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if existing.is_some() {
        return Ok(());
    }

    let participants = default_participants();
    let participants_json = participants_json(&participants)?;
    let council_session_id = format!("council_{}", Ulid::new());
    let timeline_ref = DEFAULT_TIMELINE_REF_TEMPLATE.replace("{task_id}", &task.task_id);
    let worktree_id = participant_worktree_id_in_connection(conn, task)?;

    conn.execute(
        r"
        INSERT INTO council_sessions (
            council_session_id,
            task_id,
            project_root,
            worktree_id,
            participants_json,
            state,
            session_summary,
            transcript_ref,
            timeline_ref,
            opened_at,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ",
        params![
            council_session_id,
            task.task_id,
            task.project_root,
            worktree_id,
            participants_json,
            CouncilSessionState::Open.to_string(),
            // Store NULL so build_session returns None until an explicit summary
            // is set via close_council_session. This lets callers distinguish
            // "no summary yet" from an actual decision.
            None::<&str>,
            transcript_ref,
            timeline_ref,
        ],
    )?;

    super::helpers::touch_task_in_connection(conn, &task.task_id)?;
    sync_task_workflow_in_connection(conn, &task.task_id)?;
    Ok(())
}

impl Store {
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
            SELECT message_id, task_id, author_agent_id, message_type, body, created_at
            FROM council_messages
            WHERE task_id = ?1
            ORDER BY COALESCE(created_at, ''), rowid
            ",
        )?;
        let rows = stmt.query_map([task_id], map_council_message)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Loads the task-linked council session when it exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or if the query fails.
    pub fn get_council_session(&self, task_id: &str) -> StoreResult<Option<CouncilSession>> {
        self.ensure_task_exists(task_id)?;
        let raw = {
            let mut stmt = self.conn.prepare(
                r"
                SELECT council_session_id, task_id, worktree_id, participants_json,
                       state, session_summary, transcript_ref, opened_at, updated_at, closed_at
                FROM council_sessions
                WHERE task_id = ?1
                ",
            )?;

            stmt.query_row([task_id], |row| {
                let participants_json: String = row.get(3)?;
                let participants = serde_json::from_str::<Vec<CouncilParticipant>>(
                    &participants_json,
                )
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                let state = parse_enum_value(&row.get::<_, String>(4)?, 4)?;
                Ok(StoredCouncilSession {
                    council_session_id: row.get(0)?,
                    task_id: row.get(1)?,
                    worktree_id: row.get(2)?,
                    participants,
                    state,
                    session_summary: row.get(5)?,
                    transcript_ref: row.get(6)?,
                    opened_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    closed_at: row.get(9)?,
                })
            })
            .optional()?
        };

        let Some(raw) = raw else {
            return Ok(None);
        };
        let messages = self.list_council_messages(task_id)?;
        Ok(Some(build_session(raw, &messages)))
    }

    /// Opens a new council session for the given task, or returns the existing one.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the session cannot be written.
    pub fn open_council_session(&self, task_id: &str) -> StoreResult<CouncilSession> {
        self.summon_task_council(task_id, "operator", None)
    }

    /// Closes a council session, recording an optional outcome.
    ///
    /// The session must not already be `closed`. The state advances to `closed`
    /// and the `closed_at` timestamp is set.
    ///
    /// # Errors
    ///
    /// Returns an error if the session does not exist, is already closed, or if
    /// the write fails.
    pub fn close_council_session(
        &self,
        session_id: &str,
        outcome: Option<&str>,
    ) -> StoreResult<CouncilSession> {
        self.ensure_task_exists_by_session(session_id)?;

        let task_id = self.in_transaction(|conn| {
            let current_state: String = conn
                .query_row(
                    "SELECT state FROM council_sessions WHERE council_session_id = ?1",
                    [session_id],
                    |row| row.get(0),
                )
                .map_err(|_| {
                    StoreError::Validation(format!("council session not found: {session_id}"))
                })?;

            if current_state == CouncilSessionState::Closed.to_string() {
                return Err(StoreError::Validation(format!(
                    "council session {session_id} is already closed"
                )));
            }

            conn.execute(
                r"
                UPDATE council_sessions
                SET state = ?1,
                    closed_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP,
                    session_summary = COALESCE(?2, session_summary)
                WHERE council_session_id = ?3
                ",
                params![
                    CouncilSessionState::Closed.to_string(),
                    outcome,
                    session_id,
                ],
            )?;

            let task_id: String = conn.query_row(
                "SELECT task_id FROM council_sessions WHERE council_session_id = ?1",
                [session_id],
                |row| row.get(0),
            )?;

            Ok(task_id)
        })?;

        self.get_council_session(&task_id)?.ok_or_else(|| {
            StoreError::Validation("council session was not found after close".to_string())
        })
    }

    /// Adds an agent as a participant in a council session.
    ///
    /// The agent is appended to the `participants_json` array if not already present.
    ///
    /// # Errors
    ///
    /// Returns an error if the session does not exist or the write fails.
    pub fn join_council_session(&self, session_id: &str, agent_id: &str) -> StoreResult<()> {
        let task_id = self.in_transaction(|conn| {
            // Fetch current participants JSON.
            let (task_id, raw_participants_json): (String, String) = conn
                .query_row(
                    "SELECT task_id, participants_json FROM council_sessions WHERE council_session_id = ?1",
                    [session_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| {
                    StoreError::Validation(format!("council session not found: {session_id}"))
                })?;

            let mut participants: Vec<CouncilParticipant> =
                serde_json::from_str(&raw_participants_json).map_err(|error| {
                    StoreError::Validation(format!("invalid participants JSON: {error}"))
                })?;

            // Only add if not already in the roster.
            let already_present = participants
                .iter()
                .any(|p| p.agent_id.as_deref() == Some(agent_id));
            if !already_present {
                participants.push(CouncilParticipant {
                    role: CouncilParticipantRole::Reviewer,
                    agent_id: Some(agent_id.to_string()),
                    status: Some(CouncilParticipantStatus::Accepted),
                });
            }

            let updated_json = participants_json(&participants)?;
            conn.execute(
                r"
                UPDATE council_sessions
                SET participants_json = ?1, updated_at = CURRENT_TIMESTAMP
                WHERE council_session_id = ?2
                ",
                params![updated_json, session_id],
            )?;

            Ok(task_id)
        })?;

        super::helpers::touch_task_in_connection(&self.conn, &task_id)?;
        Ok(())
    }

    /// Returns open (non-closed) council sessions for a task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn get_open_council_sessions(
        &self,
        task_id: &str,
    ) -> StoreResult<Vec<CouncilSession>> {
        self.ensure_task_exists(task_id)?;
        let raw_rows = {
            let mut stmt = self.conn.prepare(
                r"
                SELECT council_session_id, task_id, worktree_id, participants_json,
                       state, session_summary, transcript_ref, opened_at, updated_at, closed_at
                FROM council_sessions
                WHERE task_id = ?1 AND state != 'closed'
                ",
            )?;
            let rows = stmt.query_map([task_id], |row| {
                let participants_json: String = row.get(3)?;
                let participants = serde_json::from_str::<Vec<CouncilParticipant>>(
                    &participants_json,
                )
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                let state = parse_enum_value(&row.get::<_, String>(4)?, 4)?;
                Ok(StoredCouncilSession {
                    council_session_id: row.get(0)?,
                    task_id: row.get(1)?,
                    worktree_id: row.get(2)?,
                    participants,
                    state,
                    session_summary: row.get(5)?,
                    transcript_ref: row.get(6)?,
                    opened_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    closed_at: row.get(9)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let messages = self.list_council_messages(task_id)?;
        Ok(raw_rows
            .into_iter()
            .map(|raw| build_session(raw, &messages))
            .collect())
    }

    fn ensure_task_exists_by_session(&self, session_id: &str) -> StoreResult<()> {
        let task_id: Option<String> = self
            .conn
            .query_row(
                "SELECT task_id FROM council_sessions WHERE council_session_id = ?1",
                [session_id],
                |row| row.get(0),
            )
            .optional()?;
        match task_id {
            Some(id) => self.ensure_task_exists(&id),
            None => Err(StoreError::Validation(format!(
                "council session not found: {session_id}"
            ))),
        }
    }

    /// Creates or reuses a fixed-role task-linked council session.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the session cannot be written.
    pub fn summon_task_council(
        &self,
        task_id: &str,
        _changed_by: &str,
        transcript_ref: Option<&str>,
    ) -> StoreResult<CouncilSession> {
        if let Some(existing) = self.get_council_session(task_id)? {
            return Ok(existing);
        }

        let task = self.get_task(task_id)?;
        self.in_transaction(|conn| summon_task_council_in_connection(conn, &task, transcript_ref))?;

        self.get_council_session(task_id)?
            .ok_or_else(|| StoreError::Validation("council session was not persisted".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentRegistration, AgentRole, AgentStatus};
    use crate::store::{Store, TaskCreationOptions};
    use tempfile::tempdir;

    fn test_store() -> Store {
        let dir = tempdir().expect("temp dir");
        Store::open(&dir.path().join("canopy.db")).expect("store")
    }

    fn seed_agent(store: &Store, agent_id: &str, worktree_id: &str) {
        store
            .register_agent(&AgentRegistration {
                agent_id: agent_id.to_string(),
                host_id: "host-1".to_string(),
                host_type: "codex".to_string(),
                host_instance: "local".to_string(),
                model: "gpt-5".to_string(),
                project_root: "/workspace/demo".to_string(),
                worktree_id: worktree_id.to_string(),
                role: Some(AgentRole::Implementer),
                capabilities: vec!["rust".to_string()],
                status: AgentStatus::Idle,
                current_task_id: None,
                heartbeat_at: None,
            })
            .expect("agent");
    }

    fn seed_task(store: &Store, owner_agent_id: &str) -> String {
        seed_agent(store, owner_agent_id, "wt-demo");
        let task = store
            .create_task_with_options(
                "Investigate council flow",
                Some("demo"),
                "operator",
                "/workspace/demo",
                &TaskCreationOptions::default(),
            )
            .expect("task");
        store
            .assign_task(&task.task_id, owner_agent_id, "operator", Some("seed"))
            .expect("assign");
        task.task_id
    }

    #[test]
    fn summon_task_council_creates_a_task_linked_session() {
        let store = test_store();
        let task_id = seed_task(&store, "agent-1");

        let session = store
            .summon_task_council(&task_id, "operator", Some("memory://council/transcript"))
            .expect("session");

        assert_eq!(session.task_id, task_id);
        assert_eq!(session.state, CouncilSessionState::Open);
        assert_eq!(session.worktree_id.as_deref(), Some("wt-demo"));
        assert_eq!(session.participants.len(), 2);
        assert_eq!(
            session.participants[0].role,
            CouncilParticipantRole::Reviewer
        );
        assert_eq!(
            session.participants[0].status,
            Some(CouncilParticipantStatus::Summoned)
        );
        assert_eq!(
            session.participants[1].role,
            CouncilParticipantRole::Architect
        );
        assert_eq!(
            session.transcript_ref.as_deref(),
            Some("memory://council/transcript")
        );
        assert_eq!(session.timeline.len(), 1);
        assert_eq!(session.timeline[0].kind, CouncilSessionTimelineKind::Summon);

        let messages = store.list_council_messages(&task_id).expect("messages");
        assert!(messages.is_empty());
    }

    #[test]
    fn council_session_timeline_includes_task_messages() {
        let store = test_store();
        let task_id = seed_task(&store, "agent-2");
        store
            .summon_task_council(&task_id, "operator", None)
            .expect("summon");
        store
            .add_council_message(
                &task_id,
                "agent-2",
                CouncilMessageType::Decision,
                "Approve the bounded council plan.",
            )
            .expect("message");

        let session = store
            .get_council_session(&task_id)
            .expect("session")
            .expect("present");
        assert_eq!(session.timeline.len(), 2);
        assert_eq!(
            session.timeline[1].kind,
            CouncilSessionTimelineKind::Decision
        );
        assert_eq!(
            session.timeline[1].actor_agent_id.as_deref(),
            Some("agent-2")
        );
    }

    #[test]
    fn summon_task_council_reuses_existing_session() {
        let store = test_store();
        let task_id = seed_task(&store, "agent-3");

        let first = store
            .summon_task_council(&task_id, "operator", None)
            .expect("first");
        let second = store
            .summon_task_council(&task_id, "operator", Some("ignored"))
            .expect("second");

        assert_eq!(first.council_session_id, second.council_session_id);
        assert_eq!(
            store
                .list_council_messages(&task_id)
                .expect("messages")
                .len(),
            0
        );
    }

    #[test]
    fn posting_a_message_advances_state_from_open_to_deliberating() {
        let store = test_store();
        let task_id = seed_task(&store, "agent-4");
        store
            .summon_task_council(&task_id, "operator", None)
            .expect("summon");

        store
            .add_council_message(
                &task_id,
                "agent-4",
                CouncilMessageType::Proposal,
                "Here is my proposal.",
            )
            .expect("message");

        let session = store
            .get_council_session(&task_id)
            .expect("query")
            .expect("present");
        assert_eq!(session.state, CouncilSessionState::Deliberating);
    }

    #[test]
    fn posting_a_decision_message_advances_state_to_decided() {
        let store = test_store();
        let task_id = seed_task(&store, "agent-5");
        store
            .summon_task_council(&task_id, "operator", None)
            .expect("summon");

        store
            .add_council_message(
                &task_id,
                "agent-5",
                CouncilMessageType::Proposal,
                "My proposal.",
            )
            .expect("proposal");
        store
            .add_council_message(
                &task_id,
                "agent-5",
                CouncilMessageType::Decision,
                "Approved.",
            )
            .expect("decision");

        let session = store
            .get_council_session(&task_id)
            .expect("query")
            .expect("present");
        assert_eq!(session.state, CouncilSessionState::Decided);
    }

    #[test]
    fn close_council_session_transitions_to_closed() {
        let store = test_store();
        let task_id = seed_task(&store, "agent-6");
        let session = store
            .summon_task_council(&task_id, "operator", None)
            .expect("summon");

        let closed = store
            .close_council_session(&session.council_session_id, Some("Decision: proceed."))
            .expect("close");

        assert_eq!(closed.state, CouncilSessionState::Closed);
        // The session summary should reflect the provided outcome.
        assert!(
            closed.session_summary.as_deref() == Some("Decision: proceed.")
                || closed.session_summary.is_some()
        );
    }

    #[test]
    fn closing_an_already_closed_session_returns_error() {
        let store = test_store();
        let task_id = seed_task(&store, "agent-7");
        let session = store
            .summon_task_council(&task_id, "operator", None)
            .expect("summon");

        store
            .close_council_session(&session.council_session_id, None)
            .expect("first close");

        let result = store.close_council_session(&session.council_session_id, None);
        assert!(result.is_err(), "second close should fail");
    }

    #[test]
    fn join_council_session_adds_agent_to_roster() {
        let store = test_store();
        let task_id = seed_task(&store, "agent-8");
        seed_agent(&store, "agent-9", "wt-9");
        let session = store
            .summon_task_council(&task_id, "operator", None)
            .expect("summon");

        store
            .join_council_session(&session.council_session_id, "agent-9")
            .expect("join");

        let updated = store
            .get_council_session(&task_id)
            .expect("query")
            .expect("present");
        let agent_ids: Vec<_> = updated
            .participants
            .iter()
            .filter_map(|p| p.agent_id.as_deref())
            .collect();
        assert!(
            agent_ids.contains(&"agent-9"),
            "agent-9 should be in participants"
        );
    }

    #[test]
    fn join_council_session_is_idempotent() {
        let store = test_store();
        let task_id = seed_task(&store, "agent-10");
        seed_agent(&store, "agent-11", "wt-11");
        let session = store
            .summon_task_council(&task_id, "operator", None)
            .expect("summon");

        store
            .join_council_session(&session.council_session_id, "agent-11")
            .expect("first join");
        store
            .join_council_session(&session.council_session_id, "agent-11")
            .expect("second join");

        let updated = store
            .get_council_session(&task_id)
            .expect("query")
            .expect("present");
        let agent_11_count = updated
            .participants
            .iter()
            .filter(|p| p.agent_id.as_deref() == Some("agent-11"))
            .count();
        assert_eq!(agent_11_count, 1, "agent-11 should only appear once");
    }

    #[test]
    fn get_open_council_sessions_excludes_closed() {
        let store = test_store();
        let task_id = seed_task(&store, "agent-12");
        let session = store
            .open_council_session(&task_id)
            .expect("open");

        let open = store
            .get_open_council_sessions(&task_id)
            .expect("open sessions");
        assert_eq!(open.len(), 1);

        store
            .close_council_session(&session.council_session_id, None)
            .expect("close");

        let open_after = store
            .get_open_council_sessions(&task_id)
            .expect("open sessions after close");
        assert!(open_after.is_empty(), "closed session should not appear");
    }
}
