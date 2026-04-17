use crate::models::ToolAdoptionScore;
use super::StoreResult;
use rusqlite::Connection;

/// Stores a tool adoption score linked to a task.
///
/// # Errors
///
/// Returns an error if the database operation fails or if JSON serialization fails.
pub fn store_tool_adoption_score(
    conn: &Connection,
    task_id: &str,
    score: &ToolAdoptionScore,
) -> StoreResult<()> {
    let score_json = serde_json::to_string(score)?;
    conn.execute(
        "INSERT INTO tool_adoption_scores (task_id, score_json, created_at)
         VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(task_id) DO UPDATE SET score_json = excluded.score_json, created_at = excluded.created_at",
        rusqlite::params![task_id, score_json],
    )?;
    Ok(())
}

/// Loads the tool adoption score for a task, if any.
///
/// # Errors
///
/// Returns an error if the database operation fails or if JSON deserialization fails.
pub fn load_tool_adoption_score(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<Option<ToolAdoptionScore>> {
    let result = conn.query_row(
        "SELECT score_json FROM tool_adoption_scores WHERE task_id = ?1",
        rusqlite::params![task_id],
        |row| row.get::<_, String>(0),
    );
    match result {
        Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
