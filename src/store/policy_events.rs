use super::StoreResult;
use rusqlite::Connection;

pub struct PolicyEventRow<'a> {
    pub event_id: &'a str,
    pub ts_ms: i64,
    pub agent_id: &'a str,
    pub tool_name: &'a str,
    pub decision: &'a str,
    pub reason: &'a str,
    pub task_id: Option<&'a str>,
}

pub fn log_policy_event(conn: &Connection, row: &PolicyEventRow<'_>) -> StoreResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO policy_events
             (event_id, ts, agent_id, tool_name, decision, reason, task_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            row.event_id,
            row.ts_ms,
            row.agent_id,
            row.tool_name,
            row.decision,
            row.reason,
            row.task_id,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE policy_events (
                event_id TEXT PRIMARY KEY,
                ts INTEGER NOT NULL,
                agent_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                decision TEXT NOT NULL,
                reason TEXT NOT NULL,
                task_id TEXT
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn log_proceed_event_stored() {
        let conn = setup();
        log_policy_event(
            &conn,
            &PolicyEventRow {
                event_id: "01HTEST",
                ts_ms: 1_000_000,
                agent_id: "agent-1",
                tool_name: "canopy_task_list",
                decision: "proceed",
                reason: "",
                task_id: None,
            },
        )
        .unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM policy_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn log_flag_event_stored_with_reason() {
        let conn = setup();
        log_policy_event(
            &conn,
            &PolicyEventRow {
                event_id: "01HTEST2",
                ts_ms: 1_000_001,
                agent_id: "agent-2",
                tool_name: "canopy_task_delete",
                decision: "flag",
                reason: "destructive tool requires review",
                task_id: Some("task-abc"),
            },
        )
        .unwrap();
        let reason: String = conn
            .query_row(
                "SELECT reason FROM policy_events WHERE event_id = '01HTEST2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(reason, "destructive tool requires review");
    }
}
