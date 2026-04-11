use super::helpers::{
    load_task_queue_state_in_connection, load_task_review_cycle_in_connection,
    load_task_worktree_binding_in_connection,
};
use super::{Store, StoreResult};
use crate::models::TaskWorkflowContext;

impl Store {
    /// Loads the typed orchestration context for a single task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the underlying queries fail.
    pub fn get_task_workflow_context(&self, task_id: &str) -> StoreResult<TaskWorkflowContext> {
        let task = self.get_task(task_id)?;
        let queue_state = load_task_queue_state_in_connection(&self.conn, task_id)?;
        let worktree_binding = load_task_worktree_binding_in_connection(&self.conn, task_id)?;
        let review_cycle = load_task_review_cycle_in_connection(&self.conn, task_id)?;
        let council_session_id = self
            .get_council_session(task_id)?
            .map(|session| session.council_session_id);

        Ok(TaskWorkflowContext {
            task_id: task.task_id,
            queue_state,
            worktree_binding,
            review_cycle,
            council_session_id,
            execution_session_ref: task.execution_session_ref,
        })
    }

    /// Lists typed orchestration context for tasks in a project.
    ///
    /// # Errors
    ///
    /// Returns an error if the task or context queries fail.
    pub fn list_task_workflow_contexts(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskWorkflowContext>> {
        self.list_tasks_filtered(project_root, None, None)?
            .into_iter()
            .map(|task| self.get_task_workflow_context(&task.task_id))
            .collect()
    }
}
