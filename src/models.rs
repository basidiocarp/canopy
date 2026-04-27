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

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskQueueStatus {
    Queued,
    Claimed,
    Executing,
    Paused,
    Blocked,
    Review,
    Closed,
    Cancelled,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum WorktreeBindingStatus {
    Unbound,
    Bound,
    Released,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum ReviewCycleState {
    Inactive,
    Pending,
    InReview,
    DecisionReady,
    Closed,
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
pub enum CouncilParticipantRole {
    Reviewer,
    Architect,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum CouncilParticipantStatus {
    Pending,
    Summoned,
    Accepted,
    Completed,
    Declined,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum CouncilSessionState {
    Open,
    /// At least one message has been posted to the session.
    Deliberating,
    /// A decision message has been posted.
    Decided,
    Closed,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum CouncilSessionTimelineKind {
    Summon,
    Response,
    Output,
    Decision,
    Closure,
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
    CouncilSessionSummoned,
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
    SummonCouncilSession,
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
    SummonCouncilSession,
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
            Self::SummonCouncilSession => OperatorActionKind::SummonCouncilSession,
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
    DependsOn,
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
    DependsOn,
    DependencyOf,
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
    pub queue_state_id: Option<String>,
    pub worktree_binding_id: Option<String>,
    pub execution_session_ref: Option<String>,
    pub review_cycle_id: Option<String>,
    pub workflow_id: Option<String>,
    pub phase_id: Option<String>,
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
    /// When enqueuing a scoped task with the same scope as a recently completed task,
    /// this field points to the most recently completed task's ID, allowing idempotent
    /// rediscovery of prior work.
    pub prior_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskQueueStateRecord {
    pub queue_state_id: String,
    pub task_id: String,
    pub queue_name: String,
    pub lane: String,
    pub position: i64,
    pub status: TaskQueueStatus,
    pub owner_agent_id: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskWorktreeBindingRecord {
    pub worktree_binding_id: String,
    pub task_id: String,
    pub project_root: String,
    pub agent_id: Option<String>,
    pub worktree_id: Option<String>,
    pub execution_session_ref: Option<String>,
    pub status: WorktreeBindingStatus,
    pub bound_at: Option<String>,
    pub released_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskReviewCycleRecord {
    pub review_cycle_id: String,
    pub task_id: String,
    pub cycle_number: i64,
    pub state: ReviewCycleState,
    pub council_session_id: Option<String>,
    pub requested_by: Option<String>,
    pub evidence_count: i64,
    pub decision_count: i64,
    pub opened_at: Option<String>,
    pub decided_at: Option<String>,
    pub closed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskWorkflowContext {
    pub task_id: String,
    pub workflow_id: Option<String>,
    pub phase_id: Option<String>,
    pub queue_state: Option<TaskQueueStateRecord>,
    pub worktree_binding: Option<TaskWorktreeBindingRecord>,
    pub review_cycle: Option<TaskReviewCycleRecord>,
    pub council_session_id: Option<String>,
    pub execution_session_ref: Option<String>,
}

/// Describes a file-scope overlap between two tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeConflict {
    pub task_id: String,
    pub task_title: String,
    pub agent_id: String,
    pub overlapping_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    pub goal: Option<String>,
    pub next_steps: Option<String>,
    pub stop_reason: Option<String>,
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
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CouncilParticipant {
    pub role: CouncilParticipantRole,
    pub agent_id: Option<String>,
    pub status: Option<CouncilParticipantStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CouncilSessionTimelineEntry {
    pub actor_agent_id: Option<String>,
    pub body: String,
    pub created_at: Option<String>,
    pub kind: CouncilSessionTimelineKind,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CouncilSession {
    pub council_session_id: String,
    pub task_id: String,
    pub worktree_id: Option<String>,
    pub participants: Vec<CouncilParticipant>,
    pub session_summary: Option<String>,
    pub state: CouncilSessionState,
    pub timeline: Vec<CouncilSessionTimelineEntry>,
    pub transcript_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
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
    pub needs_verification_count: usize,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DriftSignals {
    /// True when correction-event rate in the last 50 evidence refs exceeds 30%.
    pub high_correction_rate: bool,
    /// Consecutive test-failure events with no intervening success.
    pub test_failure_streak: u32,
    /// Hours since the last evidence ref was attached to any active task.
    /// `None` when no evidence exists yet.
    pub evidence_gap_hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    pub drift_signals: DriftSignals,
    pub relationships: Vec<TaskRelationship>,
    pub relationship_summaries: Vec<TaskRelationshipSummary>,
    pub workflow_contexts: Vec<TaskWorkflowContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    pub council_session: Option<CouncilSession>,
    pub messages: Vec<CouncilMessage>,
    pub evidence: Vec<EvidenceRef>,
    pub relationships: Vec<TaskRelationship>,
    pub relationship_summary: TaskRelationshipSummary,
    pub workflow_context: Option<TaskWorkflowContext>,
    pub related_tasks: Vec<RelatedTask>,
    pub children: Vec<TaskSummary>,
    pub children_complete: bool,
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_adoption_score: Option<ToolAdoptionScore>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkQueueResult {
    pub schema_version: String,
    pub available_tasks: Vec<Task>,
    pub orchestration: Vec<TaskWorkflowContext>,
    pub my_role: Option<String>,
    pub my_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WhoAmIResult {
    pub schema_version: String,
    pub agent: AgentRegistration,
    pub tasks: Vec<Task>,
    pub workflow: Vec<TaskWorkflowContext>,
    pub pending_handoffs: Vec<Handoff>,
    pub file_locks: Vec<FileLock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SituationResult {
    pub schema_version: String,
    pub agents: Vec<AgentRegistration>,
    pub file_locks: Vec<FileLock>,
    pub workflow: Vec<TaskWorkflowContext>,
    pub open_handoffs_count: usize,
}

// ---------------------------------------------------------------------------
// Workflow outcome learning loop (#141g)
// ---------------------------------------------------------------------------

/// A parsed orchestration outcome record stored in Canopy's ledger.
///
/// Mirrors the `workflow-outcome-v1` septa contract. Fields are parsed at the
/// boundary from the raw JSON blob — downstream code works with typed values.
///
/// This surface is **observational only**: it records what happened so policy
/// review has a truthful baseline. It does not auto-modify routing policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowOutcomeRecord {
    pub workflow_id: String,
    pub template_id: String,
    pub handoff_path: String,
    pub terminal_status: String,
    pub failure_type: Option<String>,
    pub attempt_count: i64,
    /// JSON representation of the route taken (array of phase-role-status objects).
    pub route_taken_json: String,
    pub confidence: Option<f64>,
    pub root_cause_layer: Option<String>,
    /// JSON representation of the runtime identity context, when present.
    pub runtime_identity_json: Option<String>,
    pub started_at: String,
    pub completed_at: String,
    /// When this record was stored in Canopy.
    pub created_at: String,
}

/// One row in the outcome summary, grouped by template, failure type, and
/// the tail phase of the route taken.
///
/// This surface is **observational**: counts here describe what happened,
/// not what routing policy to apply next.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutcomeSummaryRow {
    pub template_id: String,
    pub failure_type: Option<String>,
    /// The `phase_id` of the last element in `route_taken`, or an empty
    /// string when the route was empty.
    pub last_phase: String,
    pub count: i64,
}

/// Event kind carried by a [`Notification`].
///
/// This enum will grow as new coordination signals are identified. Downstream
/// code should match against it with a wildcard arm to stay forward-compatible.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NotificationEventType {
    TaskAssigned,
    TaskCompleted,
    TaskBlocked,
    TaskCancelled,
    EvidenceReceived,
    HandoffReady,
    HandoffRejected,
    CouncilOpened,
    CouncilClosed,
}

impl std::fmt::Display for NotificationEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_string(self).unwrap_or_else(|_| format!("{self:?}"));
        write!(f, "{}", s.trim_matches('"'))
    }
}

/// A notification row stored in the `notifications` table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Notification {
    pub notification_id: String,
    pub event_type: NotificationEventType,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
    pub payload: serde_json::Value,
    pub seen: bool,
    pub created_at: String,
}

/// Status of a single tool in the adoption analysis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAdoptionStatus {
    /// Tool was called during the session.
    Used,
    /// Tool was available but not relevant to the work performed.
    AvailableUnused,
    /// Tool was relevant to the work but was not called — a gap.
    RelevantUnused,
}

/// Per-tool adoption breakdown entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolAdoptionDetail {
    pub tool_name: String,
    /// Which ecosystem component provides this tool (e.g., "hyphae", "rhizome").
    pub source: String,
    pub status: ToolAdoptionStatus,
}

/// Tool adoption score for a task session.
///
/// `Score` = `tools_used` / `tools_relevant`, or 1.0 when no tools were relevant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolAdoptionScore {
    /// Adoption ratio: 0.0 to 1.0.
    pub score: f32,
    /// Count of ecosystem tools that were actually called.
    pub tools_used: u32,
    /// Count of tools deemed relevant to the work performed.
    pub tools_relevant: u32,
    /// Count of tools that were available in the session.
    pub tools_available: u32,
    /// Per-tool breakdown.
    pub details: Vec<ToolAdoptionDetail>,
}

impl ToolAdoptionScore {
    /// Compute a score from lists of used, relevant, and available tool names.
    ///
    /// # Examples
    ///
    /// ```
    /// use canopy::models::{ToolAdoptionScore, ToolAdoptionDetail, ToolAdoptionStatus};
    ///
    /// let score = ToolAdoptionScore::compute(
    ///     &[("rhizome_get_definition", "rhizome")],
    ///     &[("rhizome_get_definition", "rhizome"), ("rhizome_find_references", "rhizome")],
    ///     &[("rhizome_get_definition", "rhizome"), ("rhizome_find_references", "rhizome"), ("hyphae_memory_recall", "hyphae")],
    /// );
    /// assert!((score.score - 0.5).abs() < 0.01);
    /// ```
    #[must_use]
    pub fn compute(
        used: &[(&str, &str)],
        relevant: &[(&str, &str)],
        available: &[(&str, &str)],
    ) -> Self {
        let tools_used = u32::try_from(used.len()).unwrap_or(u32::MAX);
        let tools_relevant = u32::try_from(relevant.len()).unwrap_or(u32::MAX);
        let tools_available = u32::try_from(available.len()).unwrap_or(u32::MAX);

        let score = if tools_relevant == 0 {
            1.0
        } else {
            let used_and_relevant = used
                .iter()
                .filter(|(name, _)| relevant.iter().any(|(rn, _)| rn == name))
                .count();
            #[allow(clippy::cast_precision_loss)]
            let score = used_and_relevant as f32 / tools_relevant as f32;
            score
        };

        let mut details = Vec::new();
        for (name, source) in available {
            let is_relevant = relevant.iter().any(|(rn, _)| rn == name);
            let is_used = used.iter().any(|(un, _)| un == name);
            let status = if is_used {
                ToolAdoptionStatus::Used
            } else if is_relevant {
                ToolAdoptionStatus::RelevantUnused
            } else {
                ToolAdoptionStatus::AvailableUnused
            };
            details.push(ToolAdoptionDetail {
                tool_name: name.to_string(),
                source: source.to_string(),
                status,
            });
        }

        Self {
            score,
            tools_used,
            tools_relevant,
            tools_available,
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolAdoptionScore, ToolAdoptionStatus, capabilities_match, parse_capabilities};

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

    #[test]
    fn score_with_no_relevant_tools_is_perfect() {
        let score = ToolAdoptionScore::compute(&[], &[], &[("hyphae_memory_recall", "hyphae")]);
        assert!((score.score - 1.0).abs() < 0.001);
    }

    #[test]
    fn score_computes_ratio_correctly() {
        let score = ToolAdoptionScore::compute(
            &[("rhizome_get_definition", "rhizome")],
            &[
                ("rhizome_get_definition", "rhizome"),
                ("rhizome_find_references", "rhizome"),
            ],
            &[
                ("rhizome_get_definition", "rhizome"),
                ("rhizome_find_references", "rhizome"),
            ],
        );
        assert!((score.score - 0.5).abs() < 0.001);
        assert_eq!(score.tools_used, 1);
        assert_eq!(score.tools_relevant, 2);
    }

    #[test]
    fn relevant_unused_tool_is_a_gap() {
        let score = ToolAdoptionScore::compute(
            &[],
            &[("rhizome_get_definition", "rhizome")],
            &[("rhizome_get_definition", "rhizome")],
        );
        assert!((score.score - 0.0).abs() < 0.001);
        assert_eq!(score.details[0].status, ToolAdoptionStatus::RelevantUnused);
    }
}
