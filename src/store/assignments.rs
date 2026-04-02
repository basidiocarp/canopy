use super::helpers::map_task_assignment;
use super::{Store, StoreError, StoreResult};
use crate::models::TaskAssignment;

impl Store {
    /// Lists assignment history globally or for one task.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_task_assignments(&self, task_id: Option<&str>) -> StoreResult<Vec<TaskAssignment>> {
        let mut assignments = Vec::new();
        if let Some(task_id) = task_id {
            let mut stmt = self.conn.prepare(
                r"
                SELECT assignment_id, task_id, assigned_to, assigned_by, reason, assigned_at
                FROM task_assignments
                WHERE task_id = ?1
                ORDER BY rowid
                ",
            )?;
            let rows = stmt.query_map([task_id], map_task_assignment)?;
            for row in rows {
                assignments.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                r"
                SELECT assignment_id, task_id, assigned_to, assigned_by, reason, assigned_at
                FROM task_assignments
                ORDER BY rowid
                ",
            )?;
            let rows = stmt.query_map([], map_task_assignment)?;
            for row in rows {
                assignments.push(row?);
            }
        }
        Ok(assignments)
    }

    /// Lists task assignments for all tasks in a project.
    ///
    /// When `project_root` is `None`, all assignments are returned.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_task_assignments_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskAssignment>> {
        if let Some(project_root) = project_root {
            let mut stmt = self.conn.prepare(
                r"
                SELECT a.assignment_id, a.task_id, a.assigned_to, a.assigned_by, a.reason, a.assigned_at
                FROM task_assignments a
                JOIN tasks t ON t.task_id = a.task_id
                WHERE t.project_root = ?1
                ORDER BY a.rowid
                ",
            )?;
            let rows = stmt.query_map([project_root], map_task_assignment)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(StoreError::from)
        } else {
            self.list_task_assignments(None)
        }
    }
}
