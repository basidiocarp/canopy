use crate::models::{
    AgentRole, AgentStatus, AttentionLevel, CouncilMessageType, EvidenceSourceKind, HandoffStatus,
    HandoffType, OperatorActionKind, SnapshotPreset, TaskPriority, TaskRelationshipRole,
    TaskSeverity, TaskSort, TaskView, VerificationState,
};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "canopy", version, about = "Task-scoped coordination runtime")]
pub struct Cli {
    /// Path to the Canopy database file
    #[arg(long, global = true)]
    pub db: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Commands {
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    ImportHandoff {
        #[arg(value_name = "PATH")]
        path: PathBuf,
        #[arg(long)]
        assign: Option<String>,
    },
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    Handoff {
        #[command(subcommand)]
        command: HandoffCommand,
    },
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommand,
    },
    Council {
        #[command(subcommand)]
        command: CouncilCommand,
    },
    Api {
        #[command(subcommand)]
        command: ApiCommand,
    },
    WorkQueue {
        #[arg(long, required = true)]
        agent_id: String,
        #[arg(long, default_value = "5")]
        limit: i64,
        #[arg(long)]
        project_root: Option<String>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Files {
        #[command(subcommand)]
        command: FilesCommand,
    },
    Situation {
        #[arg(long)]
        agent_id: Option<String>,
        #[arg(long)]
        project_root: Option<String>,
    },
    Outcome {
        #[command(subcommand)]
        command: OutcomeCommand,
    },
    Serve {
        /// Stable identifier for this agent (e.g. claude-implementer-1)
        #[arg(long, required = true)]
        agent_id: String,
        /// Scope to a project root path
        #[arg(long)]
        project: Option<String>,
        /// Git worktree identifier
        #[arg(long, default_value = "main")]
        worktree: String,
    },
    Notification {
        #[command(subcommand)]
        command: NotificationCommand,
    },
    /// Show the active MCP dispatch policy — which tool annotation classes
    /// are auto-allowed and which require operator confirmation.
    Policy {
        #[command(subcommand)]
        command: PolicyCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    Register {
        #[arg(long)]
        agent_id: String,
        #[arg(long)]
        host_id: String,
        #[arg(long)]
        host_type: String,
        #[arg(long)]
        host_instance: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        project_root: String,
        #[arg(long)]
        worktree_id: String,
        #[arg(long)]
        role: Option<AgentRole>,
        #[arg(long, value_delimiter = ',')]
        capabilities: Vec<String>,
    },
    Heartbeat {
        #[arg(long)]
        agent_id: String,
        #[arg(long)]
        status: AgentStatus,
        #[arg(long)]
        current_task_id: Option<String>,
    },
    History {
        #[arg(long)]
        agent_id: Option<String>,
        #[arg(long)]
        task_id: Option<String>,
        #[arg(long, default_value_t = 25)]
        limit: usize,
    },
    List,
}

#[derive(Debug, Subcommand)]
pub enum HandoffCommand {
    Create {
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        from_agent_id: String,
        #[arg(long)]
        to_agent_id: String,
        #[arg(long)]
        handoff_type: HandoffType,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        requested_action: Option<String>,
        /// High-level goal this handoff is working toward
        #[arg(long)]
        goal: Option<String>,
        /// Concrete next steps for the receiving agent
        #[arg(long)]
        next_steps: Option<String>,
        /// Why the current agent is stopping (e.g. completed, blocked, `needs_review`)
        #[arg(long)]
        stop_reason: Option<String>,
        #[arg(long)]
        due_at: Option<String>,
        #[arg(long)]
        expires_at: Option<String>,
    },
    Resolve {
        #[arg(long)]
        handoff_id: String,
        #[arg(long)]
        status: HandoffStatus,
        #[arg(long)]
        resolved_by: String,
        #[arg(long)]
        acting_agent_id: Option<String>,
    },
    Action {
        #[arg(long)]
        handoff_id: String,
        #[arg(long)]
        action: OperatorActionKind,
        #[arg(long)]
        changed_by: String,
        #[arg(long)]
        acting_agent_id: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    List {
        #[arg(long)]
        task_id: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum TaskCommand {
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        requested_by: String,
        #[arg(long, default_value = ".")]
        project_root: String,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        required_role: Option<AgentRole>,
        #[arg(long, value_delimiter = ',')]
        required_capabilities: Vec<String>,
        #[arg(long, default_value_t = false)]
        auto_review: bool,
        #[arg(long, default_value_t = false)]
        verification_required: bool,
        /// Comma-separated file paths or globs this task will modify
        #[arg(long, value_delimiter = ',')]
        scope: Vec<String>,
        /// Workflow instance this task belongs to
        #[arg(long)]
        workflow_id: Option<String>,
        /// Workflow phase this task is currently in
        #[arg(long)]
        phase_id: Option<String>,
    },
    Assign {
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        assigned_to: String,
        #[arg(long)]
        assigned_by: String,
        #[arg(long)]
        reason: Option<String>,
    },
    Claim {
        #[arg(long, required = true)]
        agent_id: String,
        #[arg(value_name = "TASK_ID")]
        task_id: String,
        /// Force claim despite liveness or file scope conflicts (advisory mode)
        #[arg(long = "force-claim", alias = "force")]
        force_claim: bool,
        /// Add blocking dependency on this task before claiming (sequential mode)
        #[arg(long)]
        after: Option<String>,
        /// Work in isolated git worktree
        #[arg(long)]
        worktree: bool,
    },
    Complete {
        #[arg(long, required = true)]
        agent_id: String,
        #[arg(value_name = "TASK_ID")]
        task_id: String,
        #[arg(long, required = true)]
        summary: String,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    Status {
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        status: crate::models::TaskStatus,
        #[arg(long)]
        changed_by: String,
        #[arg(long)]
        verification_state: Option<VerificationState>,
        #[arg(long)]
        blocked_reason: Option<String>,
        #[arg(long)]
        closure_summary: Option<String>,
    },
    Triage {
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        changed_by: String,
        #[arg(long)]
        priority: Option<TaskPriority>,
        #[arg(long)]
        severity: Option<TaskSeverity>,
        #[arg(long)]
        acknowledged: Option<bool>,
        #[arg(long)]
        owner_note: Option<String>,
        #[arg(long, default_value_t = false)]
        clear_owner_note: bool,
    },
    Action {
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        action: OperatorActionKind,
        #[arg(long)]
        changed_by: String,
        #[arg(long)]
        acting_agent_id: Option<String>,
        #[arg(long)]
        assigned_to: Option<String>,
        #[arg(long)]
        priority: Option<TaskPriority>,
        #[arg(long)]
        severity: Option<TaskSeverity>,
        #[arg(long)]
        verification_state: Option<VerificationState>,
        #[arg(long)]
        blocked_reason: Option<String>,
        #[arg(long)]
        closure_summary: Option<String>,
        #[arg(long)]
        owner_note: Option<String>,
        #[arg(long, default_value_t = false)]
        clear_owner_note: bool,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        from_agent_id: Option<String>,
        #[arg(long)]
        to_agent_id: Option<String>,
        #[arg(long)]
        handoff_type: Option<HandoffType>,
        #[arg(long)]
        handoff_summary: Option<String>,
        #[arg(long)]
        requested_action: Option<String>,
        #[arg(long)]
        due_at: Option<String>,
        #[arg(long)]
        review_due_at: Option<String>,
        #[arg(long)]
        expires_at: Option<String>,
        #[arg(long)]
        author_agent_id: Option<String>,
        #[arg(long)]
        message_type: Option<CouncilMessageType>,
        #[arg(long)]
        message_body: Option<String>,
        #[arg(long)]
        evidence_source_kind: Option<EvidenceSourceKind>,
        #[arg(long)]
        evidence_source_ref: Option<String>,
        #[arg(long)]
        evidence_label: Option<String>,
        #[arg(long)]
        evidence_summary: Option<String>,
        #[arg(long)]
        related_handoff_id: Option<String>,
        #[arg(long)]
        related_session_id: Option<String>,
        #[arg(long)]
        related_memory_query: Option<String>,
        #[arg(long)]
        related_symbol: Option<String>,
        #[arg(long)]
        related_file: Option<String>,
        #[arg(long)]
        follow_up_title: Option<String>,
        #[arg(long)]
        follow_up_description: Option<String>,
        #[arg(long)]
        related_task_id: Option<String>,
        #[arg(long)]
        relationship_role: Option<TaskRelationshipRole>,
    },
    Verify {
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        script: PathBuf,
        #[arg(long)]
        step: Option<String>,
    },
    List {
        #[arg(long, default_value_t = false)]
        tree: bool,
    },
    ListView {
        #[arg(long)]
        project_root: Option<String>,
        #[arg(long)]
        preset: Option<SnapshotPreset>,
        #[arg(long)]
        view: Option<TaskView>,
        #[arg(long)]
        sort: Option<TaskSort>,
        #[arg(long)]
        priority_at_least: Option<TaskPriority>,
        #[arg(long)]
        severity_at_least: Option<TaskSeverity>,
        #[arg(long)]
        acknowledged: Option<bool>,
        #[arg(long)]
        attention_at_least: Option<AttentionLevel>,
    },
    Show {
        #[arg(long)]
        task_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum CouncilCommand {
    Summon {
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        changed_by: String,
        #[arg(long)]
        transcript_ref: Option<String>,
    },
    Post {
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        author_agent_id: String,
        #[arg(long)]
        message_type: CouncilMessageType,
        #[arg(long)]
        body: String,
    },
    Show {
        #[arg(long)]
        task_id: String,
    },
    /// Open a new council session for the given task.
    Open {
        #[arg(long)]
        task: String,
    },
    /// Close an open council session, recording an optional outcome.
    Close {
        #[arg(long)]
        session: String,
        #[arg(long)]
        outcome: Option<String>,
    },
    /// List open sessions for a task with participant count and time open.
    Status {
        #[arg(long)]
        task: String,
    },
    /// Add an agent as a participant in a council session.
    Join {
        #[arg(long)]
        session: String,
        #[arg(long)]
        agent: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum EvidenceCommand {
    Add {
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        source_kind: EvidenceSourceKind,
        #[arg(long)]
        source_ref: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        related_handoff_id: Option<String>,
        #[arg(long)]
        related_session_id: Option<String>,
        #[arg(long)]
        related_memory_query: Option<String>,
        #[arg(long)]
        related_symbol: Option<String>,
        #[arg(long)]
        related_file: Option<String>,
    },
    List {
        #[arg(long)]
        task_id: String,
    },
    Verify {
        #[arg(long)]
        task_id: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ApiCommand {
    Snapshot {
        #[arg(long)]
        project_root: Option<String>,
        #[arg(long)]
        preset: Option<SnapshotPreset>,
        #[arg(long)]
        view: Option<TaskView>,
        #[arg(long)]
        sort: Option<TaskSort>,
        #[arg(long)]
        priority_at_least: Option<TaskPriority>,
        #[arg(long)]
        severity_at_least: Option<TaskSeverity>,
        #[arg(long)]
        acknowledged: Option<bool>,
        #[arg(long)]
        attention_at_least: Option<AttentionLevel>,
    },
    Task {
        #[arg(long)]
        task_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum FilesCommand {
    Lock {
        #[arg(long, required = true)]
        agent_id: String,
        #[arg(long, required = true)]
        task_id: String,
        #[arg(long, default_value = "main")]
        worktree: String,
        #[arg(trailing_var_arg = true, required = true, value_name = "FILES")]
        files: Vec<String>,
    },
    Unlock {
        #[arg(long, required = true)]
        task_id: String,
    },
    Check {
        #[arg(long)]
        agent_id: Option<String>,
        #[arg(long, default_value = "main")]
        worktree: String,
        #[arg(trailing_var_arg = true, required = true, value_name = "FILES")]
        files: Vec<String>,
    },
    List {
        #[arg(long)]
        agent_id: Option<String>,
        #[arg(long)]
        project_root: Option<String>,
    },
}

/// Subcommands for `canopy outcome`.
///
/// All commands are observational — they record and query orchestration outcomes
/// without modifying routing policy.
#[derive(Debug, clap::Subcommand)]
pub enum OutcomeCommand {
    /// Parse and store a `workflow-outcome-v1` JSON payload.
    ///
    /// Pass a path to a JSON file or `-` to read from stdin.
    Record {
        /// Path to a JSON file, or `-` to read from stdin.
        #[arg(value_name = "PATH_OR_DASH")]
        path: String,
    },
    /// List all stored outcomes, most recent first.
    List,
    /// Show a single outcome by `workflow_id`.
    Show {
        #[arg(value_name = "WORKFLOW_ID")]
        workflow_id: String,
    },
    /// Print outcome counts grouped by template, failure type, and last phase.
    Summary,
}

#[derive(Debug, Subcommand)]
pub enum NotificationCommand {
    /// List notifications, optionally including already-read ones.
    /// Usage: canopy notification list [--all]
    List {
        /// Include already-read notifications.
        #[arg(long)]
        all: bool,
    },
    /// Mark a specific notification as read.
    /// Usage: canopy notification mark-read <notification-id>
    MarkRead {
        /// Notification ID to mark as read.
        notification_id: String,
    },
    /// Mark all notifications as read.
    /// Usage: canopy notification mark-all-read
    MarkAllRead,
}

/// Subcommands for `canopy policy`.
#[derive(Debug, Subcommand)]
pub enum PolicyCommand {
    /// Print the active dispatch policy: which annotation classes are
    /// auto-allowed and which require operator confirmation.
    Show,
}
