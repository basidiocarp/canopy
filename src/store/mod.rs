mod agents;
mod assignments;
mod council;
mod events;
mod evidence;
mod files;
mod handoffs;
mod helpers;
pub mod notifications;
mod operator_actions;
mod orchestration;
mod outcomes;
mod policy_events;
mod relationships;
mod schema;
mod tasks;
pub mod tool_usage;
mod traits;

pub use policy_events::PolicyEventRow;
pub use traits::{CanopyStore, OrchestrationStore, OutcomeStore, TaskGetStore, TaskLookupStore};

use crate::models::{
    AgentHeartbeatSource, AgentRole, AgentStatus, CouncilMessageType, EvidenceSourceKind,
    ExecutionActionKind, Freshness, HandoffType, TaskAction, TaskEventType, TaskPriority,
    TaskRelationshipRole, TaskSeverity, TaskStatus, VerificationState,
};
use chrono::Utc;
use rusqlite::Connection;
use std::fs;
use std::path::Path;
use thiserror::Error;

use schema::{BASE_SCHEMA, migrate_schema};

pub(crate) const EVIDENCE_REF_SCHEMA_VERSION: &str = "1.0";
pub const CLAIM_STALE_THRESHOLD_SECS: i64 = 300;
pub const HEARTBEAT_AGING_THRESHOLD_SECS: i64 = 15 * 60;
pub const HEARTBEAT_STALE_THRESHOLD_SECS: i64 = 60 * 60;

/// Enforce capability requirements for an assign or claim operation.
///
/// Loads the task and the agent from the store and checks that the agent has
/// all capabilities the task requires. Returns `Ok(())` when the task has no
/// requirements or the agent satisfies them. Returns `Err` with a human-readable
/// message on mismatch or when the agent record is missing.
///
/// # Errors
///
/// Returns an error when the task or agent lookup fails, or when the agent
/// does not meet the task's capability requirements.
pub fn ensure_capabilities_match(
    store: &(impl CanopyStore + ?Sized),
    task_id: &str,
    agent_id: &str,
) -> StoreResult<()> {
    let task = store.get_task(task_id)?;
    if task.required_capabilities.is_empty() {
        return Ok(());
    }
    let agent = store.get_agent(agent_id).map_err(|err| {
        StoreError::Validation(format!(
            "agent not found: {err} — register this agent before assigning tasks with capability requirements"
        ))
    })?;
    let agent_caps = &agent.capabilities;
    if !crate::models::capabilities_match(agent_caps, &task.required_capabilities) {
        let missing: Vec<&str> = task
            .required_capabilities
            .iter()
            .filter(|req| !agent_caps.iter().any(|cap| cap == *req))
            .map(String::as_str)
            .collect();
        return Err(StoreError::Validation(format!(
            "capability mismatch: task requires [{}] but agent has [{}]; \
             missing: [{}] — register this agent with the required capabilities",
            task.required_capabilities.join(", "),
            agent_caps.join(", "),
            missing.join(", "),
        )));
    }
    Ok(())
}

/// Returns the age in seconds of an agent's last heartbeat.
///
/// `None` means the agent has no recorded heartbeat.
///
/// # Errors
///
/// Returns an error if the agent does not exist or the heartbeat timestamp is invalid.
pub fn agent_last_heartbeat_age_secs(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
) -> StoreResult<Option<i64>> {
    let agent = store.get_agent(agent_id)?;
    let Some(heartbeat_at) = agent.heartbeat_at.as_deref() else {
        return Ok(None);
    };

    let heartbeat_at = helpers::parse_database_timestamp(heartbeat_at)?;
    let age_secs = (Utc::now() - heartbeat_at).num_seconds().max(0);
    Ok(Some(age_secs))
}

/// Ensure an agent's last heartbeat is fresh enough to claim work.
///
/// # Errors
///
/// Returns a validation error when the agent is stale or missing a heartbeat.
pub fn ensure_agent_fresh_for_claim(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    threshold_secs: i64,
) -> StoreResult<()> {
    let age_secs = agent_last_heartbeat_age_secs(store, agent_id)?;
    match age_secs {
        Some(age_secs) if age_secs > threshold_secs => Err(StoreError::Validation(format!(
            "agent {agent_id} last heartbeat was {age_secs}s ago (threshold: {threshold_secs}s) — send a heartbeat before claiming"
        ))),
        Some(_) => Ok(()),
        None => Err(StoreError::Validation(format!(
            "agent {agent_id} has no recorded heartbeat (age: missing, threshold: {threshold_secs}s) — send a heartbeat before claiming"
        ))),
    }
}

#[must_use]
pub fn classify_agent_freshness(age_secs: Option<i64>) -> Freshness {
    match age_secs {
        Some(age_secs) if age_secs >= HEARTBEAT_STALE_THRESHOLD_SECS => Freshness::Stale,
        Some(age_secs) if age_secs >= HEARTBEAT_AGING_THRESHOLD_SECS => Freshness::Aging,
        Some(_) => Freshness::Fresh,
        None => Freshness::Missing,
    }
}

const AUTO_REVIEW_MIN_PRIORITY: TaskPriority = TaskPriority::Medium;
const AUTO_REVIEW_SUBTASKS: [(&str, &str); 3] = [
    (
        "Spec review",
        "Verify implementation matches original task spec",
    ),
    (
        "Architecture audit",
        "Check for pattern violations: WAL, atomic writes, spore usage, schema_version",
    ),
    (
        "Quality check",
        "Verify test count, clippy warnings, coverage delta",
    ),
];

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("record not found: {0}")]
    NotFound(&'static str),
    #[error("validation error: {0}")]
    Validation(String),
    /// A queued task for the same scope already exists.
    ///
    /// Callers should treat this as an idempotent duplicate: look up the
    /// existing queued task rather than creating a new one.
    #[error("duplicate queued task for scope: {scope}")]
    DuplicateQueuedTask {
        /// The scope string that caused the conflict.
        scope: String,
    },
    /// The agent has reached its concurrency cap and cannot claim another task.
    #[error("concurrency cap reached for agent {agent_id}: {claimed}/{cap} tasks claimed")]
    ConcurrencyCapReached {
        agent_id: String,
        claimed: i64,
        cap: i64,
    },
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug)]
pub struct Store {
    pub(crate) conn: Connection,
}

#[derive(Debug)]
pub(crate) struct TaskEventWrite<'a> {
    pub task_id: &'a str,
    pub event_type: TaskEventType,
    pub actor: &'a str,
    pub from_status: Option<TaskStatus>,
    pub to_status: TaskStatus,
    pub verification_state: Option<VerificationState>,
    pub owner_agent_id: Option<&'a str>,
    pub execution_action: Option<ExecutionActionKind>,
    pub execution_duration_seconds: Option<i64>,
    pub note: Option<&'a str>,
}

#[derive(Debug)]
pub(crate) struct AgentHeartbeatWrite<'a> {
    pub agent_id: &'a str,
    pub status: AgentStatus,
    pub current_task_id: Option<&'a str>,
    pub related_task_id: Option<&'a str>,
    pub source: AgentHeartbeatSource,
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

#[derive(Debug, Clone, Default)]
pub struct TaskCreationOptions {
    pub required_role: Option<AgentRole>,
    pub required_capabilities: Vec<String>,
    pub auto_review: bool,
    pub verification_required: bool,
    pub scope: Vec<String>,
    pub workflow_id: Option<String>,
    pub phase_id: Option<String>,
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
pub(crate) struct TaskOperatorActionInput<'a> {
    pub acting_agent_id: Option<&'a str>,
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
    pub review_due_at: Option<&'a str>,
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

impl<'a> From<&TaskAction<'a>> for TaskOperatorActionInput<'a> {
    #[allow(clippy::too_many_lines)]
    fn from(action: &TaskAction<'a>) -> Self {
        let mut input = Self::default();
        #[allow(clippy::match_same_arms)]
        match *action {
            TaskAction::Acknowledge { note }
            | TaskAction::Unacknowledge { note }
            | TaskAction::ClearDueAt { note }
            | TaskAction::ClearReviewDueAt { note }
            | TaskAction::Unblock { note }
            | TaskAction::ReopenWhenUnblocked { note } => input.note = note,
            TaskAction::SetPriority { priority, note } => {
                input.priority = Some(priority);
                input.note = note;
            }
            TaskAction::SetSeverity { severity, note } => {
                input.severity = Some(severity);
                input.note = note;
            }
            TaskAction::UpdateNote {
                owner_note,
                clear_owner_note,
                note,
            } => {
                input.owner_note = owner_note;
                input.clear_owner_note = clear_owner_note;
                input.note = note;
            }
            TaskAction::SetDueAt { due_at, note } => {
                input.due_at = Some(due_at);
                input.note = note;
            }
            TaskAction::SetReviewDueAt {
                review_due_at,
                note,
            } => {
                input.review_due_at = Some(review_due_at);
                input.note = note;
            }
            TaskAction::Verify {
                verification_state,
                note,
            } => {
                input.verification_state = Some(verification_state);
                input.note = note;
            }
            TaskAction::Close {
                closure_summary,
                note,
            } => {
                input.closure_summary = Some(closure_summary);
                input.note = note;
            }
            TaskAction::Block {
                blocked_reason,
                note,
            } => {
                input.blocked_reason = Some(blocked_reason);
                input.note = note;
            }
            TaskAction::Claim {
                acting_agent_id,
                note,
            }
            | TaskAction::Start {
                acting_agent_id,
                note,
            }
            | TaskAction::Resume {
                acting_agent_id,
                note,
            }
            | TaskAction::Pause {
                acting_agent_id,
                note,
            }
            | TaskAction::Yield {
                acting_agent_id,
                note,
            }
            | TaskAction::Complete {
                acting_agent_id,
                note,
            } => {
                input.acting_agent_id = Some(acting_agent_id);
                input.note = note;
            }
            TaskAction::Reassign { assigned_to, note } => {
                input.assigned_to = Some(assigned_to);
                input.note = note;
            }
            TaskAction::RecordDecision {
                author_agent_id,
                message_body,
            } => {
                input.author_agent_id = Some(author_agent_id);
                input.message_body = Some(message_body);
            }
            TaskAction::CreateHandoff {
                from_agent_id,
                to_agent_id,
                handoff_type,
                handoff_summary,
                requested_action,
                due_at,
                expires_at,
            } => {
                input.from_agent_id = Some(from_agent_id);
                input.to_agent_id = Some(to_agent_id);
                input.handoff_type = Some(handoff_type);
                input.handoff_summary = Some(handoff_summary);
                input.requested_action = requested_action;
                input.due_at = due_at;
                input.expires_at = expires_at;
            }
            TaskAction::SummonCouncilSession => {}
            TaskAction::PostCouncilMessage {
                author_agent_id,
                message_type,
                message_body,
            } => {
                input.author_agent_id = Some(author_agent_id);
                input.message_type = Some(message_type);
                input.message_body = Some(message_body);
            }
            TaskAction::AttachEvidence {
                source_kind,
                source_ref,
                label,
                summary,
                related_handoff_id,
                related_session_id,
                related_memory_query,
                related_symbol,
                related_file,
            } => {
                input.evidence_source_kind = Some(source_kind);
                input.evidence_source_ref = Some(source_ref);
                input.evidence_label = Some(label);
                input.evidence_summary = summary;
                input.related_handoff_id = related_handoff_id;
                input.related_session_id = related_session_id;
                input.related_memory_query = related_memory_query;
                input.related_symbol = related_symbol;
                input.related_file = related_file;
            }
            TaskAction::CreateFollowUp { title, description } => {
                input.follow_up_title = Some(title);
                input.follow_up_description = description;
            }
            TaskAction::LinkDependency {
                related_task_id,
                relationship_role,
            } => {
                input.related_task_id = Some(related_task_id);
                input.relationship_role = Some(relationship_role);
            }
            TaskAction::ResolveDependency { related_task_id }
            | TaskAction::PromoteFollowUp { related_task_id } => {
                input.related_task_id = Some(related_task_id);
            }
            TaskAction::CloseFollowUpChain => {}
        }
        input
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HandoffOperatorActionInput<'a> {
    pub acting_agent_id: Option<&'a str>,
    pub note: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TaskDeadlineUpdate<'a> {
    pub due_at: Option<&'a str>,
    pub clear_due_at: bool,
    pub review_due_at: Option<&'a str>,
    pub clear_review_due_at: bool,
    pub event_note: Option<&'a str>,
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
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.execute_batch(BASE_SCHEMA)?;

        migrate_schema(&conn)?;

        Ok(Self { conn })
    }

    pub(crate) fn in_transaction<T>(
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
                if let Err(rollback_err) = self.conn.execute_batch("ROLLBACK") {
                    tracing::error!(
                        original_error = %error,
                        rollback_error = %rollback_err,
                        "ROLLBACK failed after transaction error; connection may be in corrupt state"
                    );
                }
                Err(error)
            }
        }
    }

    /// Read-only transaction using BEGIN DEFERRED (no write lock).
    #[allow(dead_code)]
    fn in_read_transaction<T, F>(&self, f: F) -> StoreResult<T>
    where
        F: FnOnce(&Connection) -> StoreResult<T>,
    {
        self.conn.execute_batch("BEGIN DEFERRED")?;
        match f(&self.conn) {
            Ok(value) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback_err) = self.conn.execute_batch("ROLLBACK") {
                    tracing::error!(
                        original_error = %error,
                        rollback_error = %rollback_err,
                        "ROLLBACK failed; connection may be in corrupt state"
                    );
                }
                Err(error)
            }
        }
    }

    /// Execute a write transaction with retry on `SQLITE_BUSY`.
    /// Retries up to `max_retries` times with exponential backoff.
    pub(crate) fn in_transaction_with_retry<T, F>(&self, max_retries: u32, f: F) -> StoreResult<T>
    where
        F: Fn(&Connection) -> StoreResult<T>,
    {
        let mut attempts = 0;
        loop {
            match self.in_transaction(&f) {
                Ok(value) => return Ok(value),
                Err(ref e) if attempts < max_retries && is_busy_error(e) => {
                    attempts += 1;
                    let backoff = std::time::Duration::from_millis(50 * u64::from(attempts));
                    tracing::debug!(
                        attempt = attempts,
                        backoff_ms = backoff.as_millis(),
                        "SQLITE_BUSY, retrying"
                    );
                    std::thread::sleep(backoff);
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Lists notifications, optionally including already-seen ones.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_notifications(
        &self,
        include_seen: bool,
    ) -> StoreResult<Vec<crate::models::Notification>> {
        self.in_read_transaction(|conn| notifications::list_notifications(conn, include_seen))
    }

    /// Marks a notification as seen.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    pub fn mark_notification_seen(&self, notification_id: &str) -> StoreResult<()> {
        self.in_transaction(|conn| notifications::mark_seen(conn, notification_id))
    }

    /// Marks all unseen notifications as seen in a single update.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails.
    pub fn mark_all_notifications_seen(&self) -> StoreResult<usize> {
        self.in_transaction(|conn| {
            let count = conn.execute("UPDATE notifications SET seen = 1 WHERE seen = 0", [])?;
            Ok(count)
        })
    }
}

fn is_busy_error(e: &StoreError) -> bool {
    let msg = format!("{e}");
    msg.contains("database is locked") || msg.contains("SQLITE_BUSY")
}
