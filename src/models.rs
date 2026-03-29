use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

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
pub enum TaskView {
    All,
    Active,
    Unclaimed,
    AssignedAwaitingClaim,
    ClaimedNotStarted,
    InProgress,
    Stalled,
    PausedResumable,
    AwaitingHandoffAcceptance,
    AcceptedHandoffFollowThrough,
    Blocked,
    BlockedByDependencies,
    Review,
    ReviewWithGraphPressure,
    ReviewHandoffFollowThrough,
    Handoffs,
    FollowUpChains,
    Attention,
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
    ReviewWithGraphPressure,
    ReviewHandoffFollowThrough,
    Unclaimed,
    AssignedAwaitingClaim,
    ClaimedNotStarted,
    InProgress,
    Stalled,
    PausedResumable,
    AwaitingHandoffAcceptance,
    AcceptedHandoffFollowThrough,
    Blocked,
    BlockedByDependencies,
    Handoffs,
    FollowUpChains,
    Critical,
    Unacknowledged,
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
pub enum TaskAttentionReason {
    Blocked,
    BlockedByActiveDependency,
    BlockedByStaleDependency,
    VerificationFailed,
    ReviewRequired,
    ReviewWithGraphPressure,
    ReviewHandoffFollowThrough,
    HasOpenFollowUps,
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

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString, Display, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum TaskRelationshipKind {
    FollowUp,
    Blocks,
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
    pub created_at: String,
    pub updated_at: String,
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
    pub attention: SnapshotAttentionSummary,
    pub agents: Vec<AgentRegistration>,
    pub agent_attention: Vec<AgentAttention>,
    pub agent_heartbeat_summaries: Vec<AgentHeartbeatSummary>,
    pub heartbeats: Vec<AgentHeartbeatEvent>,
    pub tasks: Vec<Task>,
    pub task_attention: Vec<TaskAttention>,
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
    pub attention: TaskAttention,
    pub agent_attention: Vec<AgentAttention>,
    pub agent_heartbeat_summaries: Vec<AgentHeartbeatSummary>,
    pub task: Task,
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
}
