//! Outcome storage for the orchestration learning loop.
//!
//! Persists [`WorkflowOutcomeRecord`]s received from Hymenium. The raw JSON
//! blob is parsed into typed fields at the boundary; downstream code only
//! sees typed values.
//!
//! This surface is **observational only** — it records what happened so
//! policy review has a truthful baseline. It does not auto-modify routing
//! policy.

use chrono::Utc;
use rusqlite::params;

use super::{Store, StoreError, StoreResult};
use crate::models::{OutcomeSummaryRow, WorkflowOutcomeRecord};

/// Wire shape of a `workflow-outcome-v1` payload used for boundary parsing.
/// Only the fields Canopy needs to store are extracted; extras are round-tripped
/// through `route_taken_json` / `runtime_identity_json`.
#[derive(Debug, serde::Deserialize)]
struct OutcomeWireShape {
    schema_version: String,
    workflow_id: String,
    template_id: String,
    handoff_path: String,
    terminal_status: String,
    failure_type: Option<String>,
    attempt_count: i64,
    route_taken: serde_json::Value,
    confidence: Option<f64>,
    root_cause_layer: Option<String>,
    runtime_identity: Option<serde_json::Value>,
    started_at: String,
    completed_at: String,
}

impl Store {
    /// Parse and insert (or replace) a workflow outcome from a raw JSON blob.
    ///
    /// Parses required fields at the boundary. Uses `INSERT OR REPLACE` so
    /// re-recording the same `workflow_id` is safe — the latest record wins.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Validation`] when the JSON is malformed or a
    /// required field is missing. Returns a database error on write failure.
    pub fn insert_workflow_outcome(&self, raw_json: &[u8]) -> StoreResult<WorkflowOutcomeRecord> {
        let wire: OutcomeWireShape = serde_json::from_slice(raw_json).map_err(|e| {
            StoreError::Validation(format!("workflow outcome JSON is invalid: {e}"))
        })?;

        if wire.schema_version != "1.0" {
            return Err(StoreError::Validation(format!(
                "unsupported workflow outcome schema version: {} (expected 1.0)",
                wire.schema_version
            )));
        }

        let route_taken_json = serde_json::to_string(&wire.route_taken).map_err(|e| {
            StoreError::Validation(format!("route_taken serialisation failed: {e}"))
        })?;

        let runtime_identity_json = wire
            .runtime_identity
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| {
                StoreError::Validation(format!("runtime_identity serialisation failed: {e}"))
            })?;

        let created_at = Utc::now().to_rfc3339();

        self.in_transaction(|conn| {
            conn.execute(
                r"
                INSERT OR REPLACE INTO workflow_outcomes
                    (workflow_id, template_id, handoff_path, terminal_status,
                     failure_type, attempt_count, route_taken_json, confidence,
                     root_cause_layer, runtime_identity_json, started_at,
                     completed_at, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                ",
                params![
                    wire.workflow_id,
                    wire.template_id,
                    wire.handoff_path,
                    wire.terminal_status,
                    wire.failure_type,
                    wire.attempt_count,
                    route_taken_json,
                    wire.confidence,
                    wire.root_cause_layer,
                    runtime_identity_json,
                    wire.started_at,
                    wire.completed_at,
                    created_at,
                ],
            )?;
            Ok(())
        })?;

        Ok(WorkflowOutcomeRecord {
            workflow_id: wire.workflow_id,
            template_id: wire.template_id,
            handoff_path: wire.handoff_path,
            terminal_status: wire.terminal_status,
            failure_type: wire.failure_type,
            attempt_count: wire.attempt_count,
            route_taken_json,
            confidence: wire.confidence,
            root_cause_layer: wire.root_cause_layer,
            runtime_identity_json,
            started_at: wire.started_at,
            completed_at: wire.completed_at,
            created_at,
        })
    }

    /// Retrieve a single outcome by `workflow_id`.
    ///
    /// Returns `None` when the workflow has no stored outcome.
    ///
    /// # Errors
    ///
    /// Returns a database error on query failure.
    pub fn get_workflow_outcome(
        &self,
        workflow_id: &str,
    ) -> StoreResult<Option<WorkflowOutcomeRecord>> {
        let row: rusqlite::Result<WorkflowOutcomeRecord> = self.conn.query_row(
            r"
            SELECT workflow_id, template_id, handoff_path, terminal_status,
                   failure_type, attempt_count, route_taken_json, confidence,
                   root_cause_layer, runtime_identity_json, started_at,
                   completed_at, created_at
            FROM workflow_outcomes
            WHERE workflow_id = ?1
            ",
            params![workflow_id],
            map_outcome_row,
        );
        match row {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StoreError::Database(e)),
        }
    }

    /// List all stored outcomes, most recent first (by `completed_at`).
    ///
    /// # Errors
    ///
    /// Returns a database error on query failure.
    pub fn list_workflow_outcomes(&self) -> StoreResult<Vec<WorkflowOutcomeRecord>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT workflow_id, template_id, handoff_path, terminal_status,
                   failure_type, attempt_count, route_taken_json, confidence,
                   root_cause_layer, runtime_identity_json, started_at,
                   completed_at, created_at
            FROM workflow_outcomes
            ORDER BY completed_at DESC
            ",
        )?;
        let rows = stmt.query_map([], map_outcome_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Return outcome counts grouped by `(template_id, failure_type, last_phase)`.
    ///
    /// `last_phase` is the `phase_id` of the last element in `route_taken`, or
    /// an empty string when the route was empty. Rows are ordered by count
    /// descending.
    ///
    /// This surface is **observational**: counts describe what happened, not
    /// what routing policy to apply next.
    ///
    /// # Errors
    ///
    /// Returns a database error on query failure.
    pub fn outcome_summary_by_template_failure(&self) -> StoreResult<Vec<OutcomeSummaryRow>> {
        // Extract the last phase_id from route_taken_json in SQL using
        // json_extract on the last array element.
        // SQLite's json_extract supports negative array indices via the
        // json_each table-valued function; we materialise the last element
        // using a subquery to stay compatible with SQLite 3.38+.
        let mut stmt = self.conn.prepare(
            r"
            SELECT
                template_id,
                failure_type,
                COALESCE(
                    (
                        SELECT je.value ->> '$.phase_id'
                        FROM json_each(wo.route_taken_json) AS je
                        ORDER BY je.key DESC
                        LIMIT 1
                    ),
                    ''
                ) AS last_phase,
                COUNT(*) AS cnt
            FROM workflow_outcomes AS wo
            GROUP BY template_id, failure_type, last_phase
            ORDER BY cnt DESC
            ",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(OutcomeSummaryRow {
                template_id: row.get(0)?,
                failure_type: row.get(1)?,
                last_phase: row.get::<_, String>(2)?,
                count: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }
}

// ---------------------------------------------------------------------------
// Row mapper
// ---------------------------------------------------------------------------

fn map_outcome_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowOutcomeRecord> {
    Ok(WorkflowOutcomeRecord {
        workflow_id: row.get(0)?,
        template_id: row.get(1)?,
        handoff_path: row.get(2)?,
        terminal_status: row.get(3)?,
        failure_type: row.get(4)?,
        attempt_count: row.get(5)?,
        route_taken_json: row.get(6)?,
        confidence: row.get(7)?,
        root_cause_layer: row.get(8)?,
        runtime_identity_json: row.get(9)?,
        started_at: row.get(10)?,
        completed_at: row.get(11)?,
        created_at: row.get(12)?,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::path::Path;

    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("canopy_outcomes_test.db");
        let store = Store::open(Path::new(&db_path)).expect("open store");
        // Return the dir so it stays alive for the duration of the test.
        (store, dir)
    }

    fn sample_outcome_json(workflow_id: &str, template_id: &str, completed_at: &str) -> Vec<u8> {
        serde_json::json!({
            "schema_version": "1.0",
            "workflow_id": workflow_id,
            "template_id": template_id,
            "handoff_path": ".handoffs/test.md",
            "terminal_status": "completed",
            "failure_type": null,
            "attempt_count": 2,
            "route_taken": [
                {"phase_id": "implement", "role": "Worker", "status": "completed"},
                {"phase_id": "audit", "role": "Final Verifier", "status": "completed"}
            ],
            "confidence": 0.92,
            "root_cause_layer": null,
            "started_at": "2026-04-11T09:55:00Z",
            "completed_at": completed_at,
            "runtime_identity": {
                "runtime_session_id": "sess_test_001",
                "host_ref": "volva:anthropic",
                "workspace_id": null
            }
        })
        .to_string()
        .into_bytes()
    }

    fn failed_outcome_json(workflow_id: &str) -> Vec<u8> {
        serde_json::json!({
            "schema_version": "1.0",
            "workflow_id": workflow_id,
            "template_id": "impl-audit",
            "handoff_path": ".handoffs/failed-test.md",
            "terminal_status": "failed",
            "failure_type": "verification_failed",
            "attempt_count": 3,
            "route_taken": [
                {"phase_id": "implement", "role": "Worker", "status": "failed"}
            ],
            "confidence": null,
            "root_cause_layer": "execution",
            "started_at": "2026-04-11T10:00:00Z",
            "completed_at": "2026-04-11T11:00:00Z"
        })
        .to_string()
        .into_bytes()
    }

    /// Round-trip: insert a full-shape outcome and read it back, asserting field parity.
    #[test]
    fn round_trip_insert_get() {
        let (store, _dir) = temp_store();
        let wf_id = "01RTCANOPY0000000000000001";
        let raw = sample_outcome_json(wf_id, "impl-audit", "2026-04-11T14:10:00Z");

        let inserted = store.insert_workflow_outcome(&raw).expect("insert");
        let loaded = store
            .get_workflow_outcome(wf_id)
            .expect("get")
            .expect("must exist");

        assert_eq!(inserted.workflow_id, loaded.workflow_id);
        assert_eq!(loaded.template_id, "impl-audit");
        assert_eq!(loaded.terminal_status, "completed");
        assert!(loaded.failure_type.is_none());
        assert_eq!(loaded.attempt_count, 2);
        assert!((loaded.confidence.unwrap() - 0.92_f64).abs() < 1e-6);
        assert!(loaded.runtime_identity_json.is_some());
        // runtime_identity_json must contain session_id
        let id_json = loaded.runtime_identity_json.as_ref().unwrap();
        assert!(
            id_json.contains("sess_test_001"),
            "session_id must round-trip"
        );
    }

    /// List ordering: insert two outcomes with different `completed_at`, assert most-recent first.
    #[test]
    fn list_ordering_most_recent_first() {
        let (store, _dir) = temp_store();
        let raw_older = sample_outcome_json(
            "01RTCANOPY0000000000000002",
            "impl-audit",
            "2026-04-10T10:00:00Z",
        );
        let raw_newer = sample_outcome_json(
            "01RTCANOPY0000000000000003",
            "impl-audit",
            "2026-04-12T10:00:00Z",
        );

        store
            .insert_workflow_outcome(&raw_older)
            .expect("insert older");
        store
            .insert_workflow_outcome(&raw_newer)
            .expect("insert newer");

        let list = store.list_workflow_outcomes().expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(
            list[0].workflow_id, "01RTCANOPY0000000000000003",
            "most recent first"
        );
        assert_eq!(list[1].workflow_id, "01RTCANOPY0000000000000002");
    }

    /// Summary grouping: insert outcomes for multiple templates/failure types,
    /// assert the summary counts are correct.
    #[test]
    fn summary_grouping() {
        let (store, _dir) = temp_store();

        // Two completed outcomes for impl-audit with last_phase = "audit"
        store
            .insert_workflow_outcome(&sample_outcome_json(
                "01RTCANOPY0000000000000010",
                "impl-audit",
                "2026-04-11T14:10:00Z",
            ))
            .expect("insert 1");
        store
            .insert_workflow_outcome(&sample_outcome_json(
                "01RTCANOPY0000000000000011",
                "impl-audit",
                "2026-04-11T15:00:00Z",
            ))
            .expect("insert 2");

        // One failed outcome for impl-audit
        store
            .insert_workflow_outcome(&failed_outcome_json("01RTCANOPY0000000000000012"))
            .expect("insert 3");

        let rows = store
            .outcome_summary_by_template_failure()
            .expect("summary");

        // Should have two groups: completed+audit(2) and failed+verification_failed+implement(1)
        assert!(!rows.is_empty(), "summary must not be empty");

        let completed_group = rows
            .iter()
            .find(|r| r.failure_type.is_none())
            .expect("completed group");
        assert_eq!(completed_group.template_id, "impl-audit");
        assert_eq!(completed_group.last_phase, "audit");
        assert_eq!(completed_group.count, 2);

        let failed_group = rows
            .iter()
            .find(|r| r.failure_type.as_deref() == Some("verification_failed"))
            .expect("failed group");
        assert_eq!(failed_group.template_id, "impl-audit");
        assert_eq!(failed_group.last_phase, "implement");
        assert_eq!(failed_group.count, 1);
    }

    /// Invalid JSON returns a Validation error, not a database error.
    #[test]
    fn invalid_json_returns_validation_error() {
        let (store, _dir) = temp_store();
        let result = store.insert_workflow_outcome(b"not valid json");
        assert!(
            matches!(result, Err(StoreError::Validation(_))),
            "expected Validation error, got {result:?}"
        );
    }

    /// [`get_workflow_outcome`] returns `None` for unknown ids.
    #[test]
    fn get_nonexistent_returns_none() {
        let (store, _dir) = temp_store();
        let result = store
            .get_workflow_outcome("does-not-exist")
            .expect("no db error");
        assert!(result.is_none());
    }
}
