use rusqlite::params;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::helpers::{check_file_conflicts_in_connection, map_file_lock};
use super::{Store, StoreError, StoreResult};
use crate::models::FileLock;

impl Store {
    /// Lock files for an agent/task. Returns list of conflicts (files locked by other agents).
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub fn lock_files(
        &self,
        agent_id: &str,
        task_id: &str,
        files: &[String],
        worktree_id: &str,
    ) -> StoreResult<Vec<FileLock>> {
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| StoreError::Validation(error.to_string()))?;

        // Conflict check and lock acquisition run inside the same transaction
        // to prevent TOCTOU races where another agent locks between check and
        // insert. Retry on SQLITE_BUSY since lock_files is the most contention-
        // prone write operation under multi-agent load.
        self.in_transaction_with_retry(3, |conn| {
            let conflicts = check_file_conflicts_in_connection(conn, files, worktree_id, Some(agent_id))?;
            if !conflicts.is_empty() {
                return Ok(conflicts);
            }

            for file_path in files {
                let lock_id = ulid::Ulid::new().to_string();
                conn.execute(
                    r"
                    INSERT INTO file_locks (lock_id, task_id, agent_id, file_path, worktree_id, locked_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    ",
                    params![lock_id, task_id, agent_id, file_path, worktree_id, now],
                )?;
            }
            Ok(Vec::new())
        })
    }

    /// Release all file locks for a task.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub fn unlock_files(&self, task_id: &str) -> StoreResult<u64> {
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        let rows_affected = self.conn.execute(
            r"
            UPDATE file_locks
            SET released_at = ?1
            WHERE task_id = ?2 AND released_at IS NULL
            ",
            params![now, task_id],
        )?;
        Ok(u64::try_from(rows_affected).unwrap_or(0))
    }

    /// Check which files are locked by other agents (without locking).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn check_file_conflicts(
        &self,
        files: &[String],
        worktree_id: &str,
        exclude_agent_id: Option<&str>,
    ) -> StoreResult<Vec<FileLock>> {
        check_file_conflicts_in_connection(&self.conn, files, worktree_id, exclude_agent_id)
    }

    /// List active file locks, optionally filtered by `project_root` or `agent_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_file_locks(
        &self,
        project_root: Option<&str>,
        agent_id: Option<&str>,
    ) -> StoreResult<Vec<FileLock>> {
        let mut sql = String::from(
            r"
            SELECT fl.lock_id, fl.task_id, fl.agent_id, fl.file_path, fl.worktree_id,
                   fl.locked_at, fl.released_at
            FROM file_locks fl
            WHERE fl.released_at IS NULL
            ",
        );
        let mut conditions = Vec::new();
        if project_root.is_some() {
            conditions.push("EXISTS (SELECT 1 FROM tasks t WHERE t.task_id = fl.task_id AND t.project_root = ?1)");
        }
        if agent_id.is_some() {
            conditions.push(if project_root.is_some() {
                "fl.agent_id = ?2"
            } else {
                "fl.agent_id = ?1"
            });
        }
        for condition in &conditions {
            sql.push_str(" AND ");
            sql.push_str(condition);
        }
        sql.push_str(" ORDER BY fl.locked_at");

        let mut stmt = self.conn.prepare(&sql)?;
        let locks: Vec<FileLock> = match (project_root, agent_id) {
            (Some(root), Some(aid)) => {
                let rows = stmt.query_map(params![root, aid], map_file_lock)?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
            (Some(root), None) => {
                let rows = stmt.query_map(params![root], map_file_lock)?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
            (None, Some(aid)) => {
                let rows = stmt.query_map(params![aid], map_file_lock)?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
            (None, None) => {
                let rows = stmt.query_map([], map_file_lock)?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
        };
        Ok(locks)
    }
}
