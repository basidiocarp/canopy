use std::collections::HashMap;

use crate::models::{
    AgentHeartbeatEvent, AgentRegistration, AgentStatus, CouncilMessage, CouncilMessageType,
    CouncilSession, EvidenceRef, EvidenceSourceKind, FileLock, Handoff, HandoffStatus, HandoffType,
    OutcomeSummaryRow, RelatedTask, Task, TaskAction, TaskAssignment, TaskEvent, TaskRelationship,
    TaskStatus, TaskSummary, TaskWorkflowContext, WorkflowOutcomeRecord, ToolAdoptionScore,
};

use super::{EvidenceLinkRefs, HandoffTiming, StoreResult, TaskCreationOptions, TaskStatusUpdate};

#[allow(clippy::missing_errors_doc)]
pub trait AgentStore {
    fn register_agent(&self, agent: &AgentRegistration) -> StoreResult<AgentRegistration>;
    fn get_agent(&self, agent_id: &str) -> StoreResult<AgentRegistration>;
    fn heartbeat_agent(
        &self,
        agent_id: &str,
        status: AgentStatus,
        current_task_id: Option<&str>,
    ) -> StoreResult<AgentRegistration>;
    fn list_agents(&self) -> StoreResult<Vec<AgentRegistration>>;
    fn list_active_agents(&self) -> StoreResult<Vec<AgentRegistration>>;
    fn list_stale_agents(&self, stale_threshold_secs: i64) -> StoreResult<Vec<AgentRegistration>>;
    fn list_agents_filtered(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<AgentRegistration>>;
}

#[allow(clippy::missing_errors_doc)]
pub trait TaskGetStore {
    fn get_task(&self, task_id: &str) -> StoreResult<Task>;
}

#[allow(clippy::missing_errors_doc)]
pub trait TaskLookupStore: TaskGetStore {
    fn list_tasks(&self) -> StoreResult<Vec<Task>>;
    fn list_tasks_for_agent(&self, agent_id: &str) -> StoreResult<Vec<Task>>;
    fn list_tasks_filtered(
        &self,
        project_root: Option<&str>,
        status: Option<&[TaskStatus]>,
        limit: Option<i64>,
    ) -> StoreResult<Vec<Task>>;
    fn query_available_tasks(
        &self,
        role: Option<&str>,
        capabilities: &[String],
        project_root: Option<&str>,
        limit: i64,
    ) -> StoreResult<Vec<Task>>;
    fn count_tasks_by_status(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<HashMap<String, i64>>;
    fn get_children(&self, task_id: &str) -> StoreResult<Vec<TaskSummary>>;
    fn get_parent_id(&self, task_id: &str) -> StoreResult<Option<String>>;
    fn list_related_tasks(&self, task_id: &str) -> StoreResult<Vec<RelatedTask>>;
}

#[allow(clippy::missing_errors_doc, clippy::too_many_arguments)]
pub trait TaskMutationStore {
    fn create_task_with_options(
        &self,
        title: &str,
        description: Option<&str>,
        requested_by: &str,
        project_root: &str,
        options: &TaskCreationOptions,
    ) -> StoreResult<Task>;
    fn enqueue_task(
        &self,
        title: &str,
        description: Option<&str>,
        requested_by: &str,
        project_root: &str,
        options: &TaskCreationOptions,
    ) -> StoreResult<Task>;
    fn create_subtask_with_options(
        &self,
        parent_task_id: &str,
        title: &str,
        description: Option<&str>,
        requested_by: &str,
        options: &TaskCreationOptions,
    ) -> StoreResult<Task>;
    fn assign_task(
        &self,
        task_id: &str,
        assigned_to: &str,
        assigned_by: &str,
        reason: Option<&str>,
    ) -> StoreResult<Task>;
    fn update_task_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        changed_by: &str,
        update: TaskStatusUpdate<'_>,
    ) -> StoreResult<Task>;
    fn apply_task_operator_action(
        &self,
        task_id: &str,
        changed_by: &str,
        task_action: TaskAction<'_>,
    ) -> StoreResult<Task>;
    fn atomic_claim_task(&self, agent_id: &str, task_id: &str) -> StoreResult<Option<Task>>;
    fn atomic_claim_task_with_cap(
        &self,
        agent_id: &str,
        task_id: &str,
        concurrency_cap: i64,
    ) -> StoreResult<Option<Task>>;
    fn clear_task_assignment(&self, task_id: &str) -> StoreResult<()>;
}

#[allow(clippy::missing_errors_doc)]
pub trait TaskEventStore {
    fn list_task_events(&self, task_id: &str) -> StoreResult<Vec<TaskEvent>>;
    fn list_all_task_events(&self) -> StoreResult<Vec<TaskEvent>>;
    fn list_task_events_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskEvent>>;
    fn list_recent_task_events(
        &self,
        task_ids: &[String],
        limit: Option<i64>,
    ) -> StoreResult<Vec<TaskEvent>>;
}

#[allow(clippy::missing_errors_doc)]
pub trait TaskAssignmentStore {
    fn list_task_assignments(&self, task_id: Option<&str>) -> StoreResult<Vec<TaskAssignment>>;
    fn list_task_assignments_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskAssignment>>;
}

#[allow(clippy::missing_errors_doc)]
pub trait TaskRelationshipStore {
    fn list_task_relationships(&self, task_id: Option<&str>) -> StoreResult<Vec<TaskRelationship>>;
    fn list_task_relationships_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskRelationship>>;
}

#[allow(clippy::missing_errors_doc, clippy::too_many_arguments)]
pub trait HandoffStore {
    fn create_handoff(
        &self,
        task_id: &str,
        from_agent_id: &str,
        to_agent_id: &str,
        handoff_type: HandoffType,
        summary: &str,
        requested_action: Option<&str>,
        timing: HandoffTiming<'_>,
    ) -> StoreResult<Handoff>;
    fn create_handoff_with_context(
        &self,
        task_id: &str,
        from_agent_id: &str,
        to_agent_id: &str,
        handoff_type: HandoffType,
        summary: &str,
        requested_action: Option<&str>,
        goal: Option<&str>,
        next_steps: Option<&str>,
        stop_reason: Option<&str>,
        timing: HandoffTiming<'_>,
    ) -> StoreResult<Handoff>;
    fn resolve_handoff(
        &self,
        handoff_id: &str,
        status: HandoffStatus,
        resolved_by: &str,
    ) -> StoreResult<Handoff>;
    fn resolve_handoff_with_actor(
        &self,
        handoff_id: &str,
        status: HandoffStatus,
        changed_by: &str,
        acting_agent_id: Option<&str>,
    ) -> StoreResult<Handoff>;
    fn list_handoffs(&self, task_id: Option<&str>) -> StoreResult<Vec<Handoff>>;
    fn list_pending_handoffs_for(&self, agent_id: &str) -> StoreResult<Vec<Handoff>>;
    fn list_handoffs_for_project(&self, project_root: Option<&str>) -> StoreResult<Vec<Handoff>>;
    fn list_active_handoffs(&self, project_root: Option<&str>) -> StoreResult<Vec<Handoff>>;
}

#[allow(clippy::missing_errors_doc)]
pub trait FileLockStore {
    fn lock_files(
        &self,
        agent_id: &str,
        task_id: &str,
        files: &[String],
        worktree_id: &str,
    ) -> StoreResult<Vec<FileLock>>;
    fn unlock_files(&self, task_id: &str) -> StoreResult<u64>;
    fn check_file_conflicts(
        &self,
        files: &[String],
        worktree_id: &str,
        exclude_agent_id: Option<&str>,
    ) -> StoreResult<Vec<FileLock>>;
    fn list_file_locks(
        &self,
        project_root: Option<&str>,
        agent_id: Option<&str>,
    ) -> StoreResult<Vec<FileLock>>;
}

#[allow(clippy::missing_errors_doc)]
pub trait EvidenceStore {
    fn add_evidence(
        &self,
        task_id: &str,
        source_kind: EvidenceSourceKind,
        source_ref: &str,
        label: &str,
        summary: Option<&str>,
        links: EvidenceLinkRefs<'_>,
    ) -> StoreResult<EvidenceRef>;
    fn list_evidence(&self, task_id: &str) -> StoreResult<Vec<EvidenceRef>>;
    fn list_all_evidence(&self) -> StoreResult<Vec<EvidenceRef>>;
    fn list_evidence_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<EvidenceRef>>;
}

#[allow(clippy::missing_errors_doc)]
pub trait CouncilStore {
    fn add_council_message(
        &self,
        task_id: &str,
        author_agent_id: &str,
        message_type: CouncilMessageType,
        body: &str,
    ) -> StoreResult<CouncilMessage>;
    fn list_council_messages(&self, task_id: &str) -> StoreResult<Vec<CouncilMessage>>;
    fn get_council_session(&self, task_id: &str) -> StoreResult<Option<CouncilSession>>;
    fn summon_task_council(
        &self,
        task_id: &str,
        changed_by: &str,
        transcript_ref: Option<&str>,
    ) -> StoreResult<CouncilSession>;
    fn open_council_session(&self, task_id: &str) -> StoreResult<CouncilSession>;
    fn close_council_session(
        &self,
        session_id: &str,
        outcome: Option<&str>,
    ) -> StoreResult<CouncilSession>;
    fn join_council_session(&self, session_id: &str, agent_id: &str) -> StoreResult<()>;
    fn get_open_council_sessions(&self, task_id: &str) -> StoreResult<Vec<CouncilSession>>;
}

#[allow(clippy::missing_errors_doc)]
pub trait OrchestrationStore {
    fn get_task_workflow_context(&self, task_id: &str) -> StoreResult<TaskWorkflowContext>;
    fn list_task_workflow_contexts(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskWorkflowContext>>;
}

#[allow(clippy::missing_errors_doc)]
pub trait HeartbeatStore {
    fn list_agent_heartbeats(
        &self,
        agent_id: Option<&str>,
        task_id: Option<&str>,
        limit: usize,
    ) -> StoreResult<Vec<AgentHeartbeatEvent>>;
    fn list_all_agent_heartbeats(&self) -> StoreResult<Vec<AgentHeartbeatEvent>>;
    fn list_task_heartbeats(
        &self,
        task_id: &str,
        limit: usize,
    ) -> StoreResult<Vec<AgentHeartbeatEvent>>;
    fn list_agent_heartbeats_for_project(
        &self,
        project_root: Option<&str>,
        limit: Option<i64>,
    ) -> StoreResult<Vec<AgentHeartbeatEvent>>;
}

/// Store trait for orchestration outcome records (learning loop, #141g).
///
/// Observational only — these methods record and query outcomes; they do not
/// modify routing policy.
#[allow(clippy::missing_errors_doc)]
pub trait OutcomeStore {
    fn insert_workflow_outcome(&self, raw_json: &[u8]) -> StoreResult<WorkflowOutcomeRecord>;
    fn get_workflow_outcome(&self, workflow_id: &str)
    -> StoreResult<Option<WorkflowOutcomeRecord>>;
    fn list_workflow_outcomes(&self) -> StoreResult<Vec<WorkflowOutcomeRecord>>;
    fn outcome_summary_by_template_failure(&self) -> StoreResult<Vec<OutcomeSummaryRow>>;
}

/// Tool adoption scoring — records and queries tool usage events and adoption metrics.
#[allow(clippy::missing_errors_doc)]
pub trait ToolAdoptionStore {
    fn record_tool_adoption_score(
        &self,
        task_id: &str,
        score: &ToolAdoptionScore,
    ) -> StoreResult<()>;
    fn get_tool_adoption_score(
        &self,
        task_id: &str,
    ) -> StoreResult<Option<ToolAdoptionScore>>;
}

pub trait CanopyStore:
    AgentStore
    + TaskGetStore
    + TaskLookupStore
    + TaskMutationStore
    + TaskEventStore
    + TaskAssignmentStore
    + TaskRelationshipStore
    + HandoffStore
    + FileLockStore
    + EvidenceStore
    + CouncilStore
    + OrchestrationStore
    + HeartbeatStore
    + OutcomeStore
    + ToolAdoptionStore
{
}

impl<T> CanopyStore for T where
    T: AgentStore
        + TaskGetStore
        + TaskLookupStore
        + TaskMutationStore
        + TaskEventStore
        + TaskAssignmentStore
        + TaskRelationshipStore
        + HandoffStore
        + FileLockStore
        + EvidenceStore
        + CouncilStore
        + OrchestrationStore
        + HeartbeatStore
        + OutcomeStore
        + ToolAdoptionStore
{
}

impl AgentStore for super::Store {
    fn register_agent(&self, agent: &AgentRegistration) -> StoreResult<AgentRegistration> {
        self.register_agent(agent)
    }

    fn get_agent(&self, agent_id: &str) -> StoreResult<AgentRegistration> {
        self.get_agent(agent_id)
    }

    fn heartbeat_agent(
        &self,
        agent_id: &str,
        status: AgentStatus,
        current_task_id: Option<&str>,
    ) -> StoreResult<AgentRegistration> {
        self.heartbeat_agent(agent_id, status, current_task_id)
    }

    fn list_agents(&self) -> StoreResult<Vec<AgentRegistration>> {
        self.list_agents()
    }

    fn list_active_agents(&self) -> StoreResult<Vec<AgentRegistration>> {
        self.list_active_agents()
    }

    fn list_stale_agents(&self, stale_threshold_secs: i64) -> StoreResult<Vec<AgentRegistration>> {
        self.list_stale_agents(stale_threshold_secs)
    }

    fn list_agents_filtered(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<AgentRegistration>> {
        self.list_agents_filtered(project_root)
    }
}

impl TaskGetStore for super::Store {
    fn get_task(&self, task_id: &str) -> StoreResult<Task> {
        self.get_task(task_id)
    }
}

impl TaskLookupStore for super::Store {
    fn list_tasks(&self) -> StoreResult<Vec<Task>> {
        self.list_tasks()
    }

    fn list_tasks_for_agent(&self, agent_id: &str) -> StoreResult<Vec<Task>> {
        self.list_tasks_for_agent(agent_id)
    }

    fn list_tasks_filtered(
        &self,
        project_root: Option<&str>,
        status: Option<&[TaskStatus]>,
        limit: Option<i64>,
    ) -> StoreResult<Vec<Task>> {
        self.list_tasks_filtered(project_root, status, limit)
    }

    fn query_available_tasks(
        &self,
        role: Option<&str>,
        capabilities: &[String],
        project_root: Option<&str>,
        limit: i64,
    ) -> StoreResult<Vec<Task>> {
        self.query_available_tasks(role, capabilities, project_root, limit)
    }

    fn count_tasks_by_status(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<HashMap<String, i64>> {
        self.count_tasks_by_status(project_root)
    }

    fn get_children(&self, task_id: &str) -> StoreResult<Vec<TaskSummary>> {
        self.get_children(task_id)
    }

    fn get_parent_id(&self, task_id: &str) -> StoreResult<Option<String>> {
        self.get_parent_id(task_id)
    }

    fn list_related_tasks(&self, task_id: &str) -> StoreResult<Vec<RelatedTask>> {
        self.list_related_tasks(task_id)
    }
}

impl TaskMutationStore for super::Store {
    fn create_task_with_options(
        &self,
        title: &str,
        description: Option<&str>,
        requested_by: &str,
        project_root: &str,
        options: &TaskCreationOptions,
    ) -> StoreResult<Task> {
        self.create_task_with_options(title, description, requested_by, project_root, options)
    }

    fn enqueue_task(
        &self,
        title: &str,
        description: Option<&str>,
        requested_by: &str,
        project_root: &str,
        options: &TaskCreationOptions,
    ) -> StoreResult<Task> {
        self.enqueue_task(title, description, requested_by, project_root, options)
    }

    fn create_subtask_with_options(
        &self,
        parent_task_id: &str,
        title: &str,
        description: Option<&str>,
        requested_by: &str,
        options: &TaskCreationOptions,
    ) -> StoreResult<Task> {
        self.create_subtask_with_options(parent_task_id, title, description, requested_by, options)
    }

    fn assign_task(
        &self,
        task_id: &str,
        assigned_to: &str,
        assigned_by: &str,
        reason: Option<&str>,
    ) -> StoreResult<Task> {
        self.assign_task(task_id, assigned_to, assigned_by, reason)
    }

    fn update_task_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        changed_by: &str,
        update: TaskStatusUpdate<'_>,
    ) -> StoreResult<Task> {
        self.update_task_status(task_id, status, changed_by, update)
    }

    fn apply_task_operator_action(
        &self,
        task_id: &str,
        changed_by: &str,
        task_action: TaskAction<'_>,
    ) -> StoreResult<Task> {
        self.apply_task_operator_action(task_id, changed_by, task_action)
    }

    fn atomic_claim_task(&self, agent_id: &str, task_id: &str) -> StoreResult<Option<Task>> {
        self.atomic_claim_task(agent_id, task_id)
    }

    fn atomic_claim_task_with_cap(
        &self,
        agent_id: &str,
        task_id: &str,
        concurrency_cap: i64,
    ) -> StoreResult<Option<Task>> {
        self.atomic_claim_task_with_cap(agent_id, task_id, concurrency_cap)
    }

    fn clear_task_assignment(&self, task_id: &str) -> StoreResult<()> {
        self.clear_task_assignment(task_id)
    }
}

impl TaskEventStore for super::Store {
    fn list_task_events(&self, task_id: &str) -> StoreResult<Vec<TaskEvent>> {
        self.list_task_events(task_id)
    }

    fn list_all_task_events(&self) -> StoreResult<Vec<TaskEvent>> {
        self.list_all_task_events()
    }

    fn list_task_events_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskEvent>> {
        self.list_task_events_for_project(project_root)
    }

    fn list_recent_task_events(
        &self,
        task_ids: &[String],
        limit: Option<i64>,
    ) -> StoreResult<Vec<TaskEvent>> {
        self.list_recent_task_events(task_ids, limit)
    }
}

impl TaskAssignmentStore for super::Store {
    fn list_task_assignments(&self, task_id: Option<&str>) -> StoreResult<Vec<TaskAssignment>> {
        self.list_task_assignments(task_id)
    }

    fn list_task_assignments_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskAssignment>> {
        self.list_task_assignments_for_project(project_root)
    }
}

impl TaskRelationshipStore for super::Store {
    fn list_task_relationships(&self, task_id: Option<&str>) -> StoreResult<Vec<TaskRelationship>> {
        self.list_task_relationships(task_id)
    }

    fn list_task_relationships_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskRelationship>> {
        self.list_task_relationships_for_project(project_root)
    }
}

impl HandoffStore for super::Store {
    fn create_handoff(
        &self,
        task_id: &str,
        from_agent_id: &str,
        to_agent_id: &str,
        handoff_type: HandoffType,
        summary: &str,
        requested_action: Option<&str>,
        timing: HandoffTiming<'_>,
    ) -> StoreResult<Handoff> {
        self.create_handoff(
            task_id,
            from_agent_id,
            to_agent_id,
            handoff_type,
            summary,
            requested_action,
            timing,
        )
    }

    fn create_handoff_with_context(
        &self,
        task_id: &str,
        from_agent_id: &str,
        to_agent_id: &str,
        handoff_type: HandoffType,
        summary: &str,
        requested_action: Option<&str>,
        goal: Option<&str>,
        next_steps: Option<&str>,
        stop_reason: Option<&str>,
        timing: HandoffTiming<'_>,
    ) -> StoreResult<Handoff> {
        self.create_handoff_with_context(
            task_id,
            from_agent_id,
            to_agent_id,
            handoff_type,
            summary,
            requested_action,
            goal,
            next_steps,
            stop_reason,
            timing,
        )
    }

    fn resolve_handoff(
        &self,
        handoff_id: &str,
        status: HandoffStatus,
        resolved_by: &str,
    ) -> StoreResult<Handoff> {
        self.resolve_handoff(handoff_id, status, resolved_by)
    }

    fn resolve_handoff_with_actor(
        &self,
        handoff_id: &str,
        status: HandoffStatus,
        changed_by: &str,
        acting_agent_id: Option<&str>,
    ) -> StoreResult<Handoff> {
        self.resolve_handoff_with_actor(handoff_id, status, changed_by, acting_agent_id)
    }

    fn list_handoffs(&self, task_id: Option<&str>) -> StoreResult<Vec<Handoff>> {
        self.list_handoffs(task_id)
    }

    fn list_pending_handoffs_for(&self, agent_id: &str) -> StoreResult<Vec<Handoff>> {
        self.list_pending_handoffs_for(agent_id)
    }

    fn list_handoffs_for_project(&self, project_root: Option<&str>) -> StoreResult<Vec<Handoff>> {
        self.list_handoffs_for_project(project_root)
    }

    fn list_active_handoffs(&self, project_root: Option<&str>) -> StoreResult<Vec<Handoff>> {
        self.list_active_handoffs(project_root)
    }
}

impl FileLockStore for super::Store {
    fn lock_files(
        &self,
        agent_id: &str,
        task_id: &str,
        files: &[String],
        worktree_id: &str,
    ) -> StoreResult<Vec<FileLock>> {
        self.lock_files(agent_id, task_id, files, worktree_id)
    }

    fn unlock_files(&self, task_id: &str) -> StoreResult<u64> {
        self.unlock_files(task_id)
    }

    fn check_file_conflicts(
        &self,
        files: &[String],
        worktree_id: &str,
        exclude_agent_id: Option<&str>,
    ) -> StoreResult<Vec<FileLock>> {
        self.check_file_conflicts(files, worktree_id, exclude_agent_id)
    }

    fn list_file_locks(
        &self,
        project_root: Option<&str>,
        agent_id: Option<&str>,
    ) -> StoreResult<Vec<FileLock>> {
        self.list_file_locks(project_root, agent_id)
    }
}

impl EvidenceStore for super::Store {
    fn add_evidence(
        &self,
        task_id: &str,
        source_kind: EvidenceSourceKind,
        source_ref: &str,
        label: &str,
        summary: Option<&str>,
        links: EvidenceLinkRefs<'_>,
    ) -> StoreResult<EvidenceRef> {
        self.add_evidence(task_id, source_kind, source_ref, label, summary, links)
    }

    fn list_evidence(&self, task_id: &str) -> StoreResult<Vec<EvidenceRef>> {
        self.list_evidence(task_id)
    }

    fn list_all_evidence(&self) -> StoreResult<Vec<EvidenceRef>> {
        self.list_all_evidence()
    }

    fn list_evidence_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<EvidenceRef>> {
        self.list_evidence_for_project(project_root)
    }
}

impl CouncilStore for super::Store {
    fn add_council_message(
        &self,
        task_id: &str,
        author_agent_id: &str,
        message_type: CouncilMessageType,
        body: &str,
    ) -> StoreResult<CouncilMessage> {
        self.add_council_message(task_id, author_agent_id, message_type, body)
    }

    fn list_council_messages(&self, task_id: &str) -> StoreResult<Vec<CouncilMessage>> {
        self.list_council_messages(task_id)
    }

    fn get_council_session(&self, task_id: &str) -> StoreResult<Option<CouncilSession>> {
        self.get_council_session(task_id)
    }

    fn summon_task_council(
        &self,
        task_id: &str,
        changed_by: &str,
        transcript_ref: Option<&str>,
    ) -> StoreResult<CouncilSession> {
        self.summon_task_council(task_id, changed_by, transcript_ref)
    }

    fn open_council_session(&self, task_id: &str) -> StoreResult<CouncilSession> {
        self.open_council_session(task_id)
    }

    fn close_council_session(
        &self,
        session_id: &str,
        outcome: Option<&str>,
    ) -> StoreResult<CouncilSession> {
        self.close_council_session(session_id, outcome)
    }

    fn join_council_session(&self, session_id: &str, agent_id: &str) -> StoreResult<()> {
        self.join_council_session(session_id, agent_id)
    }

    fn get_open_council_sessions(&self, task_id: &str) -> StoreResult<Vec<CouncilSession>> {
        self.get_open_council_sessions(task_id)
    }
}

impl OrchestrationStore for super::Store {
    fn get_task_workflow_context(&self, task_id: &str) -> StoreResult<TaskWorkflowContext> {
        self.get_task_workflow_context(task_id)
    }

    fn list_task_workflow_contexts(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskWorkflowContext>> {
        self.list_task_workflow_contexts(project_root)
    }
}

impl HeartbeatStore for super::Store {
    fn list_agent_heartbeats(
        &self,
        agent_id: Option<&str>,
        task_id: Option<&str>,
        limit: usize,
    ) -> StoreResult<Vec<AgentHeartbeatEvent>> {
        self.list_agent_heartbeats(agent_id, task_id, limit)
    }

    fn list_all_agent_heartbeats(&self) -> StoreResult<Vec<AgentHeartbeatEvent>> {
        self.list_all_agent_heartbeats()
    }

    fn list_task_heartbeats(
        &self,
        task_id: &str,
        limit: usize,
    ) -> StoreResult<Vec<AgentHeartbeatEvent>> {
        self.list_task_heartbeats(task_id, limit)
    }

    fn list_agent_heartbeats_for_project(
        &self,
        project_root: Option<&str>,
        limit: Option<i64>,
    ) -> StoreResult<Vec<AgentHeartbeatEvent>> {
        self.list_agent_heartbeats_for_project(project_root, limit)
    }
}

impl OutcomeStore for super::Store {
    fn insert_workflow_outcome(&self, raw_json: &[u8]) -> StoreResult<WorkflowOutcomeRecord> {
        self.insert_workflow_outcome(raw_json)
    }

    fn get_workflow_outcome(
        &self,
        workflow_id: &str,
    ) -> StoreResult<Option<WorkflowOutcomeRecord>> {
        self.get_workflow_outcome(workflow_id)
    }

    fn list_workflow_outcomes(&self) -> StoreResult<Vec<WorkflowOutcomeRecord>> {
        self.list_workflow_outcomes()
    }

    fn outcome_summary_by_template_failure(&self) -> StoreResult<Vec<OutcomeSummaryRow>> {
        self.outcome_summary_by_template_failure()
    }
}

impl ToolAdoptionStore for super::Store {
    fn record_tool_adoption_score(
        &self,
        task_id: &str,
        score: &ToolAdoptionScore,
    ) -> StoreResult<()> {
        super::tool_usage::store_tool_adoption_score(&self.conn, task_id, score)
    }

    fn get_tool_adoption_score(
        &self,
        task_id: &str,
    ) -> StoreResult<Option<ToolAdoptionScore>> {
        super::tool_usage::load_tool_adoption_score(&self.conn, task_id)
    }
}
