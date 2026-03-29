use crate::models::{
    AgentStatus, AttentionLevel, CouncilMessageType, EvidenceSourceKind, HandoffStatus,
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
    },
    Action {
        #[arg(long)]
        handoff_id: String,
        #[arg(long)]
        action: OperatorActionKind,
        #[arg(long)]
        changed_by: String,
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
    List,
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
