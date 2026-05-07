use rusqlite::{Connection, params};
use ulid::Ulid;

use super::{Store, StoreError, StoreResult};
use crate::models::ReviewAnnotation;

pub(crate) fn map_review_annotation(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewAnnotation> {
    Ok(ReviewAnnotation {
        annotation_id: row.get(0)?,
        task_id: row.get(1)?,
        file_path: row.get(2)?,
        start_line: row.get(3)?,
        end_line: row.get(4)?,
        action: row.get::<_, String>(5)?.parse().map_err(|_| {
            rusqlite::Error::InvalidColumnType(5, "action".to_string(), rusqlite::types::Type::Text)
        })?,
        comment: row.get(6)?,
        anchor_hash: row.get(7)?,
        operator_id: row.get(8)?,
        created_at: row.get(9)?,
    })
}

pub(crate) fn insert_review_annotation_in_connection(
    conn: &Connection,
    task_id: &str,
    file_path: &str,
    start_line: i64,
    end_line: i64,
    action: crate::models::ReviewAnnotationAction,
    comment: &str,
    anchor_hash: &str,
    operator_id: &str,
) -> StoreResult<ReviewAnnotation> {
    let annotation_id = Ulid::new().to_string();
    conn.execute(
        r"INSERT INTO review_annotations
          (annotation_id, task_id, file_path, start_line, end_line, action, comment, anchor_hash, operator_id)
          VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            annotation_id,
            task_id,
            file_path,
            start_line,
            end_line,
            action.to_string(),
            comment,
            anchor_hash,
            operator_id,
        ],
    )?;
    conn.query_row(
        r"SELECT annotation_id, task_id, file_path, start_line, end_line, action,
                 comment, anchor_hash, operator_id, created_at
          FROM review_annotations WHERE annotation_id = ?1",
        [&annotation_id],
        map_review_annotation,
    )
    .map_err(StoreError::from)
}

pub(crate) fn list_review_annotations_for_task_in_connection(
    conn: &Connection,
    task_id: &str,
) -> StoreResult<Vec<ReviewAnnotation>> {
    let mut stmt = conn.prepare(
        r"SELECT annotation_id, task_id, file_path, start_line, end_line, action,
                 comment, anchor_hash, operator_id, created_at
          FROM review_annotations
          WHERE task_id = ?1
          ORDER BY rowid",
    )?;
    let rows = stmt.query_map([task_id], map_review_annotation)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

impl Store {
    pub fn list_review_annotations_for_task(
        &self,
        task_id: &str,
    ) -> StoreResult<Vec<ReviewAnnotation>> {
        list_review_annotations_for_task_in_connection(&self.conn, task_id)
    }
}
