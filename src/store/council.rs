use super::helpers::{add_council_message_in_connection, map_council_message};
use super::{Store, StoreError, StoreResult};
use crate::models::{CouncilMessage, CouncilMessageType};

impl Store {
    /// Appends a council message to a task thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the task or author agent does not exist or if the
    /// write fails.
    pub fn add_council_message(
        &self,
        task_id: &str,
        author_agent_id: &str,
        message_type: CouncilMessageType,
        body: &str,
    ) -> StoreResult<CouncilMessage> {
        self.in_transaction(|conn| {
            add_council_message_in_connection(conn, task_id, author_agent_id, message_type, body)
        })
    }

    /// Lists all council messages for a task in append order.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or if the query fails.
    pub fn list_council_messages(&self, task_id: &str) -> StoreResult<Vec<CouncilMessage>> {
        self.ensure_task_exists(task_id)?;
        let mut stmt = self.conn.prepare(
            r"
            SELECT message_id, task_id, author_agent_id, message_type, body
            FROM council_messages
            WHERE task_id = ?1
            ORDER BY rowid
            ",
        )?;
        let rows = stmt.query_map([task_id], map_council_message)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }
}
