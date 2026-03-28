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
pub struct Task {
    pub task_id: String,
    pub title: String,
    pub description: Option<String>,
    pub requested_by: String,
    pub project_root: String,
    pub status: TaskStatus,
    pub owner_agent_id: Option<String>,
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
    pub status: HandoffStatus,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiSnapshot {
    pub agents: Vec<AgentRegistration>,
    pub tasks: Vec<Task>,
    pub handoffs: Vec<Handoff>,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskDetail {
    pub task: Task,
    pub handoffs: Vec<Handoff>,
    pub messages: Vec<CouncilMessage>,
    pub evidence: Vec<EvidenceRef>,
}
