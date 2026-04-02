use super::helpers::{list_task_events_in_connection, map_task_event};
use super::{Store, StoreError, StoreResult};
use crate::models::TaskEvent;

impl Store {
    /// Lists timeline events for one task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn list_task_events(&self, task_id: &str) -> StoreResult<Vec<TaskEvent>> {
        self.ensure_task_exists(task_id)?;
        list_task_events_in_connection(&self.conn, task_id)
    }

    /// Lists task events across all tasks.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_all_task_events(&self) -> StoreResult<Vec<TaskEvent>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT event_id, task_id, event_type, actor, from_status, to_status,
                   verification_state, owner_agent_id, execution_action,
                   execution_duration_seconds, note, created_at
            FROM task_events
            ORDER BY rowid
            ",
        )?;
        let rows = stmt.query_map([], map_task_event)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists task events for a specific set of task IDs, with an optional row limit.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_recent_task_events(
        &self,
        task_ids: &[String],
        limit: Option<i64>,
    ) -> StoreResult<Vec<TaskEvent>> {
        if task_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: Vec<String> =
            (1..=task_ids.len()).map(|i| format!("?{i}")).collect();
        let limit_placeholder = task_ids.len() + 1;
        let limit_clause = if limit.is_some() {
            format!("LIMIT ?{limit_placeholder}")
        } else {
            String::new()
        };
        let sql = format!(
            r"
            SELECT event_id, task_id, event_type, actor, from_status, to_status,
                   verification_state, owner_agent_id, execution_action,
                   execution_duration_seconds, note, created_at
            FROM task_events
            WHERE task_id IN ({})
            ORDER BY rowid
            {limit_clause}
            ",
            placeholders.join(", ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        for (i, task_id) in task_ids.iter().enumerate() {
            stmt.raw_bind_parameter(i + 1, task_id.as_str())?;
        }
        if let Some(lim) = limit {
            stmt.raw_bind_parameter(limit_placeholder, lim)?;
        }
        let mut rows = stmt.raw_query();
        let mut events = Vec::new();
        while let Some(row) = rows.next()? {
            events.push(map_task_event(row)?);
        }
        Ok(events)
    }

    /// Lists task events for all tasks in a project.
    ///
    /// When `project_root` is `None`, all task events are returned (equivalent
    /// to [`list_all_task_events`](Self::list_all_task_events)).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_task_events_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskEvent>> {
        if let Some(project_root) = project_root {
            let mut stmt = self.conn.prepare(
                r"
                SELECT e.event_id, e.task_id, e.event_type, e.actor, e.from_status, e.to_status,
                       e.verification_state, e.owner_agent_id, e.execution_action,
                       e.execution_duration_seconds, e.note, e.created_at
                FROM task_events e
                JOIN tasks t ON t.task_id = e.task_id
                WHERE t.project_root = ?1
                ORDER BY e.rowid
                ",
            )?;
            let rows = stmt.query_map([project_root], map_task_event)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(StoreError::from)
        } else {
            self.list_all_task_events()
        }
    }
}
