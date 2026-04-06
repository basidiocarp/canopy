use std::collections::HashMap;

use crate::models::{
    AgentHeartbeatEvent, AgentRegistration, AgentStatus, CouncilMessage, CouncilMessageType,
    EvidenceRef, EvidenceSourceKind, FileLock, Handoff, HandoffStatus, HandoffType, RelatedTask,
    Task, TaskAction, TaskAssignment, TaskEvent, TaskRelationship, TaskStatus, TaskSummary,
};

use super::{EvidenceLinkRefs, HandoffTiming, StoreResult, TaskCreationOptions, TaskStatusUpdate};

/// Trait covering all store operations used by tools, api, and MCP server.
///
/// Extracted from `Store` to enable mock implementations for unit testing.
/// The concrete `Store` implements this trait by delegating to its existing methods.
#[allow(clippy::missing_errors_doc, clippy::too_many_arguments)]
pub trait CanopyStore {
    // --- Agent operations ---

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

    // --- Task operations ---

    fn get_task(&self, task_id: &str) -> StoreResult<Task>;

    fn create_task_with_options(
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

    fn list_tasks(&self) -> StoreResult<Vec<Task>>;

    fn list_tasks_for_agent(&self, agent_id: &str) -> StoreResult<Vec<Task>>;

    fn list_tasks_filtered(
        &self,
        project_root: Option<&str>,
        status: Option<&[TaskStatus]>,
        limit: Option<i64>,
    ) -> StoreResult<Vec<Task>>;

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

    fn query_available_tasks(
        &self,
        role: Option<&str>,
        capabilities: &[String],
        project_root: Option<&str>,
        limit: i64,
    ) -> StoreResult<Vec<Task>>;

    fn clear_task_assignment(&self, task_id: &str) -> StoreResult<()>;

    fn count_tasks_by_status(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<HashMap<String, i64>>;

    fn get_children(&self, task_id: &str) -> StoreResult<Vec<TaskSummary>>;

    fn get_parent_id(&self, task_id: &str) -> StoreResult<Option<String>>;

    fn list_related_tasks(&self, task_id: &str) -> StoreResult<Vec<RelatedTask>>;

    // --- Task events ---

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

    // --- Task assignments ---

    fn list_task_assignments(&self, task_id: Option<&str>) -> StoreResult<Vec<TaskAssignment>>;

    fn list_task_assignments_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskAssignment>>;

    // --- Task relationships ---

    fn list_task_relationships(&self, task_id: Option<&str>) -> StoreResult<Vec<TaskRelationship>>;

    fn list_task_relationships_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskRelationship>>;

    // --- Handoff operations ---

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

    // --- File lock operations ---

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

    // --- Evidence operations ---

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

    // --- Council operations ---

    fn add_council_message(
        &self,
        task_id: &str,
        author_agent_id: &str,
        message_type: CouncilMessageType,
        body: &str,
    ) -> StoreResult<CouncilMessage>;

    fn list_council_messages(&self, task_id: &str) -> StoreResult<Vec<CouncilMessage>>;

    // --- Heartbeat operations ---

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

/// Blanket implementation: `Store` implements `CanopyStore` via its existing methods.
/// This macro-free approach delegates each trait method to the inherent `Store` method.
impl CanopyStore for super::Store {
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

    fn get_task(&self, task_id: &str) -> StoreResult<Task> {
        self.get_task(task_id)
    }

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

    fn query_available_tasks(
        &self,
        role: Option<&str>,
        capabilities: &[String],
        project_root: Option<&str>,
        limit: i64,
    ) -> StoreResult<Vec<Task>> {
        self.query_available_tasks(role, capabilities, project_root, limit)
    }

    fn clear_task_assignment(&self, task_id: &str) -> StoreResult<()> {
        self.clear_task_assignment(task_id)
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

    fn list_task_assignments(&self, task_id: Option<&str>) -> StoreResult<Vec<TaskAssignment>> {
        self.list_task_assignments(task_id)
    }

    fn list_task_assignments_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskAssignment>> {
        self.list_task_assignments_for_project(project_root)
    }

    fn list_task_relationships(&self, task_id: Option<&str>) -> StoreResult<Vec<TaskRelationship>> {
        self.list_task_relationships(task_id)
    }

    fn list_task_relationships_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskRelationship>> {
        self.list_task_relationships_for_project(project_root)
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{TaskPriority, TaskSeverity, TaskStatus, VerificationState};
    use crate::store::StoreError;

    /// Minimal mock that implements only the methods needed for `tool_task_get`.
    /// All other methods panic — tests should only call the methods they mock.
    struct MockStore {
        tasks: Vec<Task>,
    }

    impl MockStore {
        fn with_tasks(tasks: Vec<Task>) -> Self {
            Self { tasks }
        }
    }

    /// Helper to build a minimal Task for testing.
    fn make_task(id: &str, title: &str) -> Task {
        Task {
            task_id: id.to_string(),
            title: title.to_string(),
            description: None,
            requested_by: "test".to_string(),
            project_root: ".".to_string(),
            parent_task_id: None,
            required_role: None,
            required_capabilities: Vec::new(),
            auto_review: false,
            verification_required: false,
            status: TaskStatus::Open,
            verification_state: VerificationState::Unknown,
            priority: TaskPriority::Medium,
            severity: TaskSeverity::None,
            owner_agent_id: None,
            owner_note: None,
            acknowledged_by: None,
            acknowledged_at: None,
            blocked_reason: None,
            verified_by: None,
            verified_at: None,
            closed_by: None,
            closure_summary: None,
            closed_at: None,
            due_at: None,
            review_due_at: None,
            scope: Vec::new(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
        }
    }

    /// Macro to generate panicking stubs for all trait methods except the ones
    /// we explicitly implement. This keeps the mock focused and the test small.
    macro_rules! stub_canopy_store {
        ($($method:ident ($($arg:ident : $ty:ty),*) -> $ret:ty;)*) => {
            $(
                fn $method(&self, $($arg: $ty),*) -> $ret {
                    let _ = ($(&$arg),*);
                    unimplemented!(concat!("MockStore::", stringify!($method), " not implemented for this test"))
                }
            )*
        };
    }

    impl CanopyStore for MockStore {
        fn get_task(&self, task_id: &str) -> StoreResult<Task> {
            self.tasks
                .iter()
                .find(|t| t.task_id == task_id)
                .cloned()
                .ok_or(StoreError::NotFound("task"))
        }

        // Stubs for all other required methods
        stub_canopy_store! {
            register_agent(agent: &AgentRegistration) -> StoreResult<AgentRegistration>;
            get_agent(agent_id: &str) -> StoreResult<AgentRegistration>;
            heartbeat_agent(agent_id: &str, status: AgentStatus, current_task_id: Option<&str>) -> StoreResult<AgentRegistration>;
            list_agents() -> StoreResult<Vec<AgentRegistration>>;
            list_active_agents() -> StoreResult<Vec<AgentRegistration>>;
            list_stale_agents(threshold: i64) -> StoreResult<Vec<AgentRegistration>>;
            list_agents_filtered(project_root: Option<&str>) -> StoreResult<Vec<AgentRegistration>>;
            create_task_with_options(title: &str, description: Option<&str>, requested_by: &str, project_root: &str, options: &TaskCreationOptions) -> StoreResult<Task>;
            create_subtask_with_options(parent_task_id: &str, title: &str, description: Option<&str>, requested_by: &str, options: &TaskCreationOptions) -> StoreResult<Task>;
            assign_task(task_id: &str, assigned_to: &str, assigned_by: &str, reason: Option<&str>) -> StoreResult<Task>;
            list_tasks() -> StoreResult<Vec<Task>>;
            list_tasks_for_agent(agent_id: &str) -> StoreResult<Vec<Task>>;
            list_tasks_filtered(project_root: Option<&str>, status: Option<&[TaskStatus]>, limit: Option<i64>) -> StoreResult<Vec<Task>>;
            update_task_status(task_id: &str, status: TaskStatus, changed_by: &str, update: TaskStatusUpdate<'_>) -> StoreResult<Task>;
            apply_task_operator_action(task_id: &str, changed_by: &str, task_action: TaskAction<'_>) -> StoreResult<Task>;
            atomic_claim_task(agent_id: &str, task_id: &str) -> StoreResult<Option<Task>>;
            query_available_tasks(role: Option<&str>, capabilities: &[String], project_root: Option<&str>, limit: i64) -> StoreResult<Vec<Task>>;
            clear_task_assignment(task_id: &str) -> StoreResult<()>;
            count_tasks_by_status(project_root: Option<&str>) -> StoreResult<HashMap<String, i64>>;
            get_children(task_id: &str) -> StoreResult<Vec<TaskSummary>>;
            get_parent_id(task_id: &str) -> StoreResult<Option<String>>;
            list_related_tasks(task_id: &str) -> StoreResult<Vec<RelatedTask>>;
            list_task_events(task_id: &str) -> StoreResult<Vec<TaskEvent>>;
            list_all_task_events() -> StoreResult<Vec<TaskEvent>>;
            list_task_events_for_project(project_root: Option<&str>) -> StoreResult<Vec<TaskEvent>>;
            list_recent_task_events(task_ids: &[String], limit: Option<i64>) -> StoreResult<Vec<TaskEvent>>;
            list_task_assignments(task_id: Option<&str>) -> StoreResult<Vec<TaskAssignment>>;
            list_task_assignments_for_project(project_root: Option<&str>) -> StoreResult<Vec<TaskAssignment>>;
            list_task_relationships(task_id: Option<&str>) -> StoreResult<Vec<TaskRelationship>>;
            list_task_relationships_for_project(project_root: Option<&str>) -> StoreResult<Vec<TaskRelationship>>;
            create_handoff(task_id: &str, from_agent_id: &str, to_agent_id: &str, handoff_type: HandoffType, summary: &str, requested_action: Option<&str>, timing: HandoffTiming<'_>) -> StoreResult<Handoff>;
            resolve_handoff(handoff_id: &str, status: HandoffStatus, resolved_by: &str) -> StoreResult<Handoff>;
            resolve_handoff_with_actor(handoff_id: &str, status: HandoffStatus, changed_by: &str, acting_agent_id: Option<&str>) -> StoreResult<Handoff>;
            list_handoffs(task_id: Option<&str>) -> StoreResult<Vec<Handoff>>;
            list_pending_handoffs_for(agent_id: &str) -> StoreResult<Vec<Handoff>>;
            list_handoffs_for_project(project_root: Option<&str>) -> StoreResult<Vec<Handoff>>;
            list_active_handoffs(project_root: Option<&str>) -> StoreResult<Vec<Handoff>>;
            lock_files(agent_id: &str, task_id: &str, files: &[String], worktree_id: &str) -> StoreResult<Vec<FileLock>>;
            unlock_files(task_id: &str) -> StoreResult<u64>;
            check_file_conflicts(files: &[String], worktree_id: &str, exclude_agent_id: Option<&str>) -> StoreResult<Vec<FileLock>>;
            list_file_locks(project_root: Option<&str>, agent_id: Option<&str>) -> StoreResult<Vec<FileLock>>;
            add_evidence(task_id: &str, source_kind: EvidenceSourceKind, source_ref: &str, label: &str, summary: Option<&str>, links: EvidenceLinkRefs<'_>) -> StoreResult<EvidenceRef>;
            list_evidence(task_id: &str) -> StoreResult<Vec<EvidenceRef>>;
            list_all_evidence() -> StoreResult<Vec<EvidenceRef>>;
            list_evidence_for_project(project_root: Option<&str>) -> StoreResult<Vec<EvidenceRef>>;
            add_council_message(task_id: &str, author_agent_id: &str, message_type: CouncilMessageType, body: &str) -> StoreResult<CouncilMessage>;
            list_council_messages(task_id: &str) -> StoreResult<Vec<CouncilMessage>>;
            list_agent_heartbeats(agent_id: Option<&str>, task_id: Option<&str>, limit: usize) -> StoreResult<Vec<AgentHeartbeatEvent>>;
            list_all_agent_heartbeats() -> StoreResult<Vec<AgentHeartbeatEvent>>;
            list_task_heartbeats(task_id: &str, limit: usize) -> StoreResult<Vec<AgentHeartbeatEvent>>;
            list_agent_heartbeats_for_project(project_root: Option<&str>, limit: Option<i64>) -> StoreResult<Vec<AgentHeartbeatEvent>>;
        }
    }

    #[test]
    fn mock_store_enables_tool_unit_testing() {
        use crate::tools;
        use serde_json::json;

        let store = MockStore::with_tasks(vec![
            make_task("task-001", "Fix the widget"),
            make_task("task-002", "Update docs"),
        ]);

        // tool_task_get works with the mock — no SQLite needed
        let result = tools::task::tool_task_get(&store, "agent-1", &json!({"task_id": "task-001"}));
        assert!(!result.is_error, "expected success, got error");
        let text = &result.content[0].text;
        assert!(
            text.contains("Fix the widget"),
            "response should contain task title"
        );

        // Missing task returns an error result, not a panic
        let result =
            tools::task::tool_task_get(&store, "agent-1", &json!({"task_id": "nonexistent"}));
        assert!(result.is_error, "expected error for missing task");
    }
}
