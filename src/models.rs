use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

#[must_use]
pub fn parse_capabilities(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

#[must_use]
pub fn capabilities_match(agent: &[String], required: &[String]) -> bool {
    if required.is_empty() || agent.is_empty() {
        return true;
    }

    required.iter().all(|required_capability| {
        agent
            .iter()
            .any(|agent_capability| agent_capability == required_capability)
    })
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Assigned,
    InProgress,
    Blocked,
    ReviewRequired,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum AgentRole {
    Orchestrator,
    Implementer,
    Validator,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum AgentHeartbeatSource {
    Register,
    Heartbeat,
    TaskSync,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    Assigned,
    InProgress,
    Blocked,
    // blocked_reason and child_task_id are surfaced on the task record.
    ReviewRequired,
    Completed,
    Closed,
    Cancelled,
}

impl TaskStatus {
    #[must_use]
    pub fn allowed_transitions(self) -> &'static [Self] {
        match self {
            Self::Open => &[
                Self::Assigned,
                Self::InProgress,
                Self::Blocked,
                Self::ReviewRequired,
                Self::Cancelled,
            ],
            Self::Assigned => &[
                Self::Open,
                Self::InProgress,
                Self::Blocked,
                Self::ReviewRequired,
                Self::Cancelled,
            ],
            Self::InProgress => &[
                Self::Blocked,
                Self::ReviewRequired,
                Self::Completed,
                Self::Cancelled,
            ],
            Self::Blocked => &[
                Self::Open,
                Self::Assigned,
                Self::InProgress,
                Self::ReviewRequired,
                Self::Cancelled,
            ],
            Self::ReviewRequired => &[
                Self::Blocked,
                Self::InProgress,
                Self::Completed,
                Self::Closed,
                Self::Cancelled,
            ],
            Self::Completed => &[Self::Closed, Self::Open],
            Self::Closed | Self::Cancelled => &[Self::Open],
        }
    }
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskView {
    All,
    Active,
    Unclaimed,
    AssignedAwaitingClaim,
    ClaimedNotStarted,
    InProgress,
    Stalled,
    PausedResumable,
    DueSoon,
    DueSoonExecution,
    DueSoonReview,
    OverdueExecution,
    OverdueExecutionOwned,
    OverdueExecutionUnclaimed,
    OverdueReview,
    AwaitingHandoffAcceptance,
    DueSoonHandoffAcceptance,
    OverdueHandoffAcceptance,
    AcceptedHandoffFollowThrough,
    DueSoonAcceptedHandoffFollowThrough,
    OverdueAcceptedHandoffFollowThrough,
    Blocked,
    BlockedByDependencies,
    Review,
    DueSoonReviewHandoffFollowThrough,
    OverdueReviewHandoffFollowThrough,
    DueSoonReviewDecisionFollowThrough,
    OverdueReviewDecisionFollowThrough,
    ReviewWithGraphPressure,
    ReviewHandoffFollowThrough,
    ReviewDecisionFollowThrough,
    ReviewAwaitingSupport,
    ReviewReadyForDecision,
    ReviewReadyForCloseout,
    Handoffs,
    FollowUpChains,
    Attention,
    FileConflicts,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum SnapshotPreset {
    Default,
    Attention,
    ReviewQueue,
    DueSoonReviewHandoffFollowThrough,
    OverdueReviewHandoffFollowThrough,
    DueSoonReviewDecisionFollowThrough,
    OverdueReviewDecisionFollowThrough,
    ReviewWithGraphPressure,
    ReviewHandoffFollowThrough,
    ReviewDecisionFollowThrough,
    ReviewAwaitingSupport,
    ReviewReadyForDecision,
    ReviewReadyForCloseout,
    Unclaimed,
    AssignedAwaitingClaim,
    ClaimedNotStarted,
    InProgress,
    Stalled,
    PausedResumable,
    DueSoon,
    DueSoonExecution,
    DueSoonReview,
    OverdueExecution,
    OverdueExecutionOwned,
    OverdueExecutionUnclaimed,
    OverdueReview,
    AwaitingHandoffAcceptance,
    DueSoonHandoffAcceptance,
    OverdueHandoffAcceptance,
    AcceptedHandoffFollowThrough,
    DueSoonAcceptedHandoffFollowThrough,
    OverdueAcceptedHandoffFollowThrough,
    Blocked,
    BlockedByDependencies,
    Handoffs,
    FollowUpChains,
    Critical,
    Unacknowledged,
    FileConflicts,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskSort {
    Status,
    Title,
    UpdatedAt,
    CreatedAt,
    Verification,
    Priority,
    Severity,
    Attention,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskSeverity {
    None,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum VerificationState {
    Unknown,
    Pending,
    Passed,
    Failed,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum HandoffType {
    RequestHelp,
    RequestReview,
    TransferOwnership,
    RequestVerification,
    RecordDecision,
    CloseTask,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum HandoffStatus {
    Open,
    Accepted,
    Rejected,
    Expired,
    Cancelled,
    Completed,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum CouncilMessageType {
    Proposal,
    Objection,
    Evidence,
    Decision,
    Handoff,
    Status,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum EvidenceSourceKind {
    HyphaeSession,
    HyphaeRecall,
    HyphaeOutcome,
    CortinaEvent,
    MyceliumCommand,
    MyceliumExplain,
    RhizomeImpact,
    RhizomeExport,
    ScriptVerification,
    ManualNote,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskEventType {
    Created,
    Assigned,
    OwnershipTransferred,
    StatusChanged,
    ExecutionUpdated,
    TriageUpdated,
    DeadlineUpdated,
    RelationshipUpdated,
    HandoffCreated,
    HandoffUpdated,
    CouncilMessagePosted,
    EvidenceAttached,
    FollowUpTaskCreated,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum ExecutionActionKind {
    ClaimTask,
    StartTask,
    ResumeTask,
    PauseTask,
    YieldTask,
    CompleteTask,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AttentionLevel {
    Normal,
    NeedsAttention,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Freshness {
    Fresh,
    Aging,
    Stale,
    Missing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum BreachSeverity {
    None,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TaskAttentionReason {
    Blocked,
    BlockedByActiveDependency,
    BlockedByStaleDependency,
    DueSoonExecution,
    OverdueExecution,
    DueSoonReview,
    OverdueReview,
    VerificationFailed,
    ReviewRequired,
    ReviewWithGraphPressure,
    ReviewHandoffFollowThrough,
    ReviewDecisionFollowThrough,
    ReviewAwaitingSupport,
    ReviewReadyForDecision,
    ReviewReadyForCloseout,
    HasOpenFollowUps,
    AllChildrenComplete,
    AssignedAwaitingClaim,
    ClaimedNotStarted,
    PausedResumable,
    AwaitingHandoffAcceptance,
    AcceptedHandoffPendingExecution,
    Unacknowledged,
    HighPriority,
    CriticalPriority,
    HighSeverity,
    CriticalSeverity,
    AgingUpdate,
    StaleUpdate,
    AgingOwnerHeartbeat,
    StaleOwnerHeartbeat,
    MissingOwnerHeartbeat,
    AgingOpenHandoff,
    StaleOpenHandoff,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HandoffAttentionReason {
    AgingOpenHandoff,
    StaleOpenHandoff,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum OperatorActionKind {
    AcknowledgeTask,
    UnacknowledgeTask,
    VerifyTask,
    RecordDecision,
    CloseTask,
    ReassignTask,
    ClaimTask,
    StartTask,
    ResumeTask,
    PauseTask,
    YieldTask,
    CompleteTask,
    ResolveDependency,
    ReopenBlockedTaskWhenUnblocked,
    PromoteFollowUp,
    CloseFollowUpChain,
    SetTaskPriority,
    SetTaskSeverity,
    BlockTask,
    UnblockTask,
    UpdateTaskNote,
    SetTaskDueAt,
    ClearTaskDueAt,
    SetReviewDueAt,
    ClearReviewDueAt,
    CreateHandoff,
    PostCouncilMessage,
    AttachEvidence,
    CreateFollowUpTask,
    LinkTaskDependency,
    AcceptHandoff,
    RejectHandoff,
    CancelHandoff,
    CompleteHandoff,
    FollowUpHandoff,
    ExpireHandoff,
}

/// Typed task action enum that replaces the `(OperatorActionKind, TaskOperatorActionInput)` pair.
///
/// Each variant carries only the fields its action requires, providing compile-time
/// safety against passing wrong field combinations. The `action_kind` method maps
/// each variant back to the `OperatorActionKind` used for event recording.
#[derive(Debug, Clone, Copy)]
pub enum TaskAction<'a> {
    Acknowledge {
        note: Option<&'a str>,
    },
    Unacknowledge {
        note: Option<&'a str>,
    },
    SetPriority {
        priority: TaskPriority,
        note: Option<&'a str>,
    },
    SetSeverity {
        severity: TaskSeverity,
        note: Option<&'a str>,
    },
    UpdateNote {
        owner_note: Option<&'a str>,
        clear_owner_note: bool,
        note: Option<&'a str>,
    },
    SetDueAt {
        due_at: &'a str,
        note: Option<&'a str>,
    },
    ClearDueAt {
        note: Option<&'a str>,
    },
    SetReviewDueAt {
        review_due_at: &'a str,
        note: Option<&'a str>,
    },
    ClearReviewDueAt {
        note: Option<&'a str>,
    },
    Verify {
        verification_state: VerificationState,
        note: Option<&'a str>,
    },
    Close {
        closure_summary: &'a str,
        note: Option<&'a str>,
    },
    Block {
        blocked_reason: &'a str,
        note: Option<&'a str>,
    },
    Unblock {
        note: Option<&'a str>,
    },
    ReopenWhenUnblocked {
        note: Option<&'a str>,
    },
    Claim {
        acting_agent_id: &'a str,
        note: Option<&'a str>,
    },
    Start {
        acting_agent_id: &'a str,
        note: Option<&'a str>,
    },
    Resume {
        acting_agent_id: &'a str,
        note: Option<&'a str>,
    },
    Pause {
        acting_agent_id: &'a str,
        note: Option<&'a str>,
    },
    Yield {
        acting_agent_id: &'a str,
        note: Option<&'a str>,
    },
    Complete {
        acting_agent_id: &'a str,
        note: Option<&'a str>,
    },
    Reassign {
        assigned_to: &'a str,
        note: Option<&'a str>,
    },
    RecordDecision {
        author_agent_id: &'a str,
        message_body: &'a str,
    },
    CreateHandoff {
        from_agent_id: &'a str,
        to_agent_id: &'a str,
        handoff_type: HandoffType,
        handoff_summary: &'a str,
        requested_action: Option<&'a str>,
        due_at: Option<&'a str>,
        expires_at: Option<&'a str>,
    },
    PostCouncilMessage {
        author_agent_id: &'a str,
        message_type: CouncilMessageType,
        message_body: &'a str,
    },
    AttachEvidence {
        source_kind: EvidenceSourceKind,
        source_ref: &'a str,
        label: &'a str,
        summary: Option<&'a str>,
        related_handoff_id: Option<&'a str>,
        related_session_id: Option<&'a str>,
        related_memory_query: Option<&'a str>,
        related_symbol: Option<&'a str>,
        related_file: Option<&'a str>,
    },
    CreateFollowUp {
        title: &'a str,
        description: Option<&'a str>,
    },
    LinkDependency {
        related_task_id: &'a str,
        relationship_role: TaskRelationshipRole,
    },
    ResolveDependency {
        related_task_id: &'a str,
    },
    PromoteFollowUp {
        related_task_id: &'a str,
    },
    CloseFollowUpChain,
}

impl TaskAction<'_> {
    /// Maps each variant to its corresponding `OperatorActionKind` for event recording.
    #[must_use]
    pub fn action_kind(&self) -> OperatorActionKind {
        match self {
            Self::Acknowledge { .. } => OperatorActionKind::AcknowledgeTask,
            Self::Unacknowledge { .. } => OperatorActionKind::UnacknowledgeTask,
            Self::SetPriority { .. } => OperatorActionKind::SetTaskPriority,
            Self::SetSeverity { .. } => OperatorActionKind::SetTaskSeverity,
            Self::UpdateNote { .. } => OperatorActionKind::UpdateTaskNote,
            Self::SetDueAt { .. } => OperatorActionKind::SetTaskDueAt,
            Self::ClearDueAt { .. } => OperatorActionKind::ClearTaskDueAt,
            Self::SetReviewDueAt { .. } => OperatorActionKind::SetReviewDueAt,
            Self::ClearReviewDueAt { .. } => OperatorActionKind::ClearReviewDueAt,
            Self::Verify { .. } => OperatorActionKind::VerifyTask,
            Self::Close { .. } => OperatorActionKind::CloseTask,
            Self::Block { .. } => OperatorActionKind::BlockTask,
            Self::Unblock { .. } => OperatorActionKind::UnblockTask,
            Self::ReopenWhenUnblocked { .. } => OperatorActionKind::ReopenBlockedTaskWhenUnblocked,
            Self::Claim { .. } => OperatorActionKind::ClaimTask,
            Self::Start { .. } => OperatorActionKind::StartTask,
            Self::Resume { .. } => OperatorActionKind::ResumeTask,
            Self::Pause { .. } => OperatorActionKind::PauseTask,
            Self::Yield { .. } => OperatorActionKind::YieldTask,
            Self::Complete { .. } => OperatorActionKind::CompleteTask,
            Self::Reassign { .. } => OperatorActionKind::ReassignTask,
            Self::RecordDecision { .. } => OperatorActionKind::RecordDecision,
            Self::CreateHandoff { .. } => OperatorActionKind::CreateHandoff,
            Self::PostCouncilMessage { .. } => OperatorActionKind::PostCouncilMessage,
            Self::AttachEvidence { .. } => OperatorActionKind::AttachEvidence,
            Self::CreateFollowUp { .. } => OperatorActionKind::CreateFollowUpTask,
            Self::LinkDependency { .. } => OperatorActionKind::LinkTaskDependency,
            Self::ResolveDependency { .. } => OperatorActionKind::ResolveDependency,
            Self::PromoteFollowUp { .. } => OperatorActionKind::PromoteFollowUp,
            Self::CloseFollowUpChain => OperatorActionKind::CloseFollowUpChain,
        }
    }
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskRelationshipKind {
    FollowUp,
    Blocks,
    Parent,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskRelationshipRole {
    FollowUpParent,
    FollowUpChild,
    Blocks,
    BlockedBy,
    Parent,
    Child,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OperatorActionTargetKind {
    Task,
    Handoff,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AgentAttentionReason {
    AgingHeartbeat,
    StaleHeartbeat,
    MissingHeartbeat,
    BlockedStatus,
    ReviewRequiredStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRegistration {
    pub agent_id: String,
    pub host_id: String,
    pub host_type: String,
    pub host_instance: String,
    pub model: String,
    pub project_root: String,
    pub worktree_id: String,
    pub role: Option<AgentRole>,
    pub capabilities: Vec<String>,
    pub status: AgentStatus,
    pub current_task_id: Option<String>,
    pub heartbeat_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentHeartbeatEvent {
    pub heartbeat_id: String,
    pub agent_id: String,
    pub status: AgentStatus,
    pub current_task_id: Option<String>,
    pub related_task_id: Option<String>,
    pub source: AgentHeartbeatSource,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub task_id: String,
    pub title: String,
    pub description: Option<String>,
    pub requested_by: String,
    pub project_root: String,
    pub parent_task_id: Option<String>,
    pub required_role: Option<AgentRole>,
    pub required_capabilities: Vec<String>,
    pub auto_review: bool,
    pub verification_required: bool,
    pub status: TaskStatus,
    pub verification_state: VerificationState,
    pub priority: TaskPriority,
    pub severity: TaskSeverity,
    pub owner_agent_id: Option<String>,
    pub owner_note: Option<String>,
    pub acknowledged_by: Option<String>,
    pub acknowledged_at: Option<String>,
    pub blocked_reason: Option<String>,
    pub verified_by: Option<String>,
    pub verified_at: Option<String>,
    pub closed_by: Option<String>,
    pub closure_summary: Option<String>,
    pub closed_at: Option<String>,
    pub due_at: Option<String>,
    pub review_due_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// File paths or globs this task will modify
    pub scope: Vec<String>,
}

/// Describes a file-scope overlap between two tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeConflict {
    pub task_id: String,
    pub task_title: String,
    pub agent_id: String,
    pub overlapping_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLock {
    pub lock_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub file_path: String,
    pub worktree_id: String,
    pub locked_at: String,
    pub released_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskSummary {
    pub task_id: String,
    pub title: String,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Handoff {
    pub handoff_id: String,
    pub task_id: String,
    pub from_agent_id: String,
    pub to_agent_id: String,
    pub handoff_type: HandoffType,
    pub summary: String,
    pub requested_action: Option<String>,
    pub due_at: Option<String>,
    pub expires_at: Option<String>,
    pub status: HandoffStatus,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskAssignment {
    pub assignment_id: String,
    pub task_id: String,
    pub assigned_to: String,
    pub assigned_by: String,
    pub reason: Option<String>,
    pub assigned_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CouncilMessage {
    pub message_id: String,
    pub task_id: String,
    pub author_agent_id: String,
    pub message_type: CouncilMessageType,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceRef {
    pub schema_version: String,
    pub evidence_id: String,
    pub task_id: String,
    pub source_kind: EvidenceSourceKind,
    pub source_ref: String,
    pub label: String,
    pub summary: Option<String>,
    pub related_handoff_id: Option<String>,
    pub related_session_id: Option<String>,
    pub related_memory_query: Option<String>,
    pub related_symbol: Option<String>,
    pub related_file: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceVerificationStatus {
    Verified,
    Failed,
    Stale,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceVerificationResult {
    pub evidence_id: String,
    pub task_id: String,
    pub source_kind: EvidenceSourceKind,
    pub source_ref: String,
    pub status: EvidenceVerificationStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceVerificationReport {
    pub schema_version: String,
    pub results: Vec<EvidenceVerificationResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEvent {
    pub event_id: String,
    pub task_id: String,
    pub event_type: TaskEventType,
    pub actor: String,
    pub from_status: Option<TaskStatus>,
    pub to_status: TaskStatus,
    pub verification_state: Option<VerificationState>,
    pub owner_agent_id: Option<String>,
    pub execution_action: Option<ExecutionActionKind>,
    pub execution_duration_seconds: Option<i64>,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviewCycleContext {
    pub has_evidence: bool,
    pub has_council_message: bool,
    pub has_council_decision: bool,
}

#[must_use]
pub fn event_note_value<'a>(note: Option<&'a str>, key: &str) -> Option<&'a str> {
    note.and_then(|note| {
        note.split("; ").find_map(|segment| {
            let (entry_key, value) = segment.split_once('=')?;
            (entry_key.trim() == key).then_some(value.trim())
        })
    })
}

#[must_use]
pub fn council_message_type_from_event_note(note: Option<&str>) -> Option<&str> {
    event_note_value(note, "message_type")
}

pub fn derive_review_cycle_context<'a, I>(task_events: I) -> ReviewCycleContext
where
    I: IntoIterator<Item = &'a TaskEvent>,
{
    let task_events = task_events.into_iter().collect::<Vec<_>>();
    let review_cycle_start_index = task_events
        .iter()
        .rposition(|event| {
            event.event_type == TaskEventType::StatusChanged
                && event.to_status == TaskStatus::ReviewRequired
        })
        .unwrap_or(0);

    let mut context = ReviewCycleContext::default();
    for event in task_events.into_iter().skip(review_cycle_start_index) {
        match event.event_type {
            TaskEventType::EvidenceAttached => {
                context.has_evidence = true;
            }
            TaskEventType::CouncilMessagePosted => {
                context.has_council_message = true;
                if council_message_type_from_event_note(event.note.as_deref()) == Some("decision") {
                    context.has_council_decision = true;
                }
            }
            _ => {}
        }
    }

    context
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRelationship {
    pub relationship_id: String,
    pub source_task_id: String,
    pub target_task_id: String,
    pub kind: TaskRelationshipKind,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelatedTask {
    pub relationship_id: String,
    pub relationship_kind: TaskRelationshipKind,
    pub relationship_role: TaskRelationshipRole,
    pub related_task_id: String,
    pub title: String,
    pub status: TaskStatus,
    pub verification_state: VerificationState,
    pub priority: TaskPriority,
    pub severity: TaskSeverity,
    pub owner_agent_id: Option<String>,
    pub blocked_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentAttention {
    pub agent_id: String,
    pub level: AttentionLevel,
    pub freshness: Freshness,
    pub last_heartbeat_at: Option<String>,
    pub current_task_id: Option<String>,
    pub reasons: Vec<AgentAttentionReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffAttention {
    pub handoff_id: String,
    pub task_id: String,
    pub level: AttentionLevel,
    pub freshness: Freshness,
    pub reasons: Vec<HandoffAttentionReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskAttention {
    pub task_id: String,
    pub level: AttentionLevel,
    pub freshness: Freshness,
    pub acknowledged: bool,
    pub owner_heartbeat_freshness: Option<Freshness>,
    pub open_handoff_freshness: Option<Freshness>,
    pub reasons: Vec<TaskAttentionReason>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DeadlineState {
    None,
    Scheduled,
    DueSoon,
    Overdue,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TaskDeadlineKind {
    Execution,
    Review,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskDeadlineSummary {
    pub task_id: String,
    pub due_at: Option<String>,
    pub review_due_at: Option<String>,
    pub execution_state: DeadlineState,
    pub review_state: DeadlineState,
    pub active_deadline_kind: Option<TaskDeadlineKind>,
    pub active_deadline_at: Option<String>,
    pub active_deadline_state: DeadlineState,
    pub due_in_seconds: Option<i64>,
    pub overdue_by_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskSlaSummary {
    pub task_id: String,
    pub due_soon_count: usize,
    pub overdue_count: usize,
    pub oldest_overdue_seconds: Option<i64>,
    pub highest_risk_queue: Option<SnapshotPreset>,
    pub breach_severity: BreachSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRelationshipSummary {
    pub task_id: String,
    pub blocker_count: usize,
    pub active_blocker_count: usize,
    pub stale_blocker_count: usize,
    pub blocking_count: usize,
    pub follow_up_parent_count: usize,
    pub follow_up_child_count: usize,
    pub open_follow_up_child_count: usize,
    pub parent_count: usize,
    pub child_count: usize,
    pub open_child_count: usize,
    pub children_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotAttentionSummary {
    pub tasks_needing_attention: usize,
    pub critical_tasks: usize,
    pub handoffs_needing_attention: usize,
    pub stale_handoffs: usize,
    pub agents_needing_attention: usize,
    pub stale_agents: usize,
    pub actionable_tasks: usize,
    pub actionable_handoffs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotSlaSummary {
    pub due_soon_count: usize,
    pub overdue_count: usize,
    pub oldest_overdue_seconds: Option<i64>,
    pub breach_severity: BreachSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskOwnershipSummary {
    pub task_id: String,
    pub current_owner_agent_id: Option<String>,
    pub assignment_count: usize,
    pub reassignment_count: usize,
    pub last_assigned_to: Option<String>,
    pub last_assigned_by: Option<String>,
    pub last_assigned_at: Option<String>,
    pub last_assignment_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskHeartbeatSummary {
    pub task_id: String,
    pub heartbeat_count: usize,
    pub related_agent_count: usize,
    pub fresh_agents: usize,
    pub aging_agents: usize,
    pub stale_agents: usize,
    pub missing_agents: usize,
    pub last_heartbeat_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskExecutionSummary {
    pub task_id: String,
    pub claim_count: usize,
    pub run_count: usize,
    pub pause_count: usize,
    pub yield_count: usize,
    pub completion_count: usize,
    pub claimed_at: Option<String>,
    pub started_at: Option<String>,
    pub last_execution_at: Option<String>,
    pub last_execution_action: Option<ExecutionActionKind>,
    pub last_execution_agent_id: Option<String>,
    pub total_execution_seconds: i64,
    pub active_execution_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentHeartbeatSummary {
    pub agent_id: String,
    pub current_task_id: Option<String>,
    pub heartbeat_count: usize,
    pub last_heartbeat_at: Option<String>,
    pub last_status: Option<AgentStatus>,
    pub freshness: Freshness,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorAction {
    pub action_id: String,
    pub kind: OperatorActionKind,
    pub target_kind: OperatorActionTargetKind,
    pub level: AttentionLevel,
    pub task_id: Option<String>,
    pub handoff_id: Option<String>,
    pub agent_id: Option<String>,
    pub title: String,
    pub summary: String,
    pub due_at: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiSnapshot {
    pub schema_version: String,
    pub attention: SnapshotAttentionSummary,
    pub sla_summary: SnapshotSlaSummary,
    pub agents: Vec<AgentRegistration>,
    pub agent_attention: Vec<AgentAttention>,
    pub agent_heartbeat_summaries: Vec<AgentHeartbeatSummary>,
    pub heartbeats: Vec<AgentHeartbeatEvent>,
    pub tasks: Vec<Task>,
    pub task_attention: Vec<TaskAttention>,
    pub task_sla_summaries: Vec<TaskSlaSummary>,
    pub deadline_summaries: Vec<TaskDeadlineSummary>,
    pub task_heartbeat_summaries: Vec<TaskHeartbeatSummary>,
    pub execution_summaries: Vec<TaskExecutionSummary>,
    pub ownership: Vec<TaskOwnershipSummary>,
    pub handoffs: Vec<Handoff>,
    pub handoff_attention: Vec<HandoffAttention>,
    pub operator_actions: Vec<OperatorAction>,
    pub evidence: Vec<EvidenceRef>,
    pub relationships: Vec<TaskRelationship>,
    pub relationship_summaries: Vec<TaskRelationshipSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskDetail {
    pub schema_version: String,
    pub attention: TaskAttention,
    pub sla_summary: TaskSlaSummary,
    pub agent_attention: Vec<AgentAttention>,
    pub agent_heartbeat_summaries: Vec<AgentHeartbeatSummary>,
    pub task: Task,
    pub deadline_summary: TaskDeadlineSummary,
    pub ownership: TaskOwnershipSummary,
    pub heartbeat_summary: TaskHeartbeatSummary,
    pub execution_summary: TaskExecutionSummary,
    pub assignments: Vec<TaskAssignment>,
    pub events: Vec<TaskEvent>,
    pub heartbeats: Vec<AgentHeartbeatEvent>,
    pub handoffs: Vec<Handoff>,
    pub handoff_attention: Vec<HandoffAttention>,
    pub operator_actions: Vec<OperatorAction>,
    pub allowed_actions: Vec<OperatorAction>,
    pub messages: Vec<CouncilMessage>,
    pub evidence: Vec<EvidenceRef>,
    pub relationships: Vec<TaskRelationship>,
    pub relationship_summary: TaskRelationshipSummary,
    pub related_tasks: Vec<RelatedTask>,
    pub children: Vec<TaskSummary>,
    pub children_complete: bool,
    pub parent_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{capabilities_match, parse_capabilities};

    #[test]
    fn parse_capabilities_reads_json_arrays_and_falls_back_to_empty() {
        assert_eq!(
            parse_capabilities(r#"["rust","hyphae","sqlite"]"#),
            vec![
                "rust".to_string(),
                "hyphae".to_string(),
                "sqlite".to_string()
            ]
        );
        assert!(parse_capabilities("not-json").is_empty());
    }

    #[test]
    fn capabilities_match_uses_all_required_with_backward_compatible_empty_lists() {
        assert!(capabilities_match(
            &["rust".to_string(), "hyphae".to_string()],
            &["rust".to_string()]
        ));
        assert!(!capabilities_match(
            &["rust".to_string()],
            &["rust".to_string(), "sqlite".to_string()]
        ));
        assert!(capabilities_match(&[], &["rust".to_string()]));
        assert!(capabilities_match(&["rust".to_string()], &[]));
        assert!(!capabilities_match(
            &["rust".to_string()],
            &["Rust".to_string()]
        ));
    }
}
