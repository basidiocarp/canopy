use super::helpers::{add_evidence_in_connection, map_evidence};
use super::{EvidenceLinkRefs, Store, StoreError, StoreResult};
use crate::models::{EvidenceRef, EvidenceSourceKind};

impl Store {
    /// Attaches an evidence reference to a task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist, a related handoff is
    /// missing, or the write fails.
    pub fn add_evidence(
        &self,
        task_id: &str,
        source_kind: EvidenceSourceKind,
        source_ref: &str,
        label: &str,
        summary: Option<&str>,
        links: EvidenceLinkRefs<'_>,
    ) -> StoreResult<EvidenceRef> {
        self.in_transaction(|conn| {
            add_evidence_in_connection(
                conn,
                task_id,
                source_kind,
                source_ref,
                label,
                summary,
                links,
            )
        })
    }

    /// Lists evidence refs for one task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn list_evidence(&self, task_id: &str) -> StoreResult<Vec<EvidenceRef>> {
        self.ensure_task_exists(task_id)?;
        let mut stmt = self.conn.prepare(
            r"
            SELECT evidence_id, task_id, source_kind, source_ref, label, summary, related_handoff_id,
                   related_session_id, related_memory_query, related_symbol, related_file, schema_version
            FROM evidence_refs
            WHERE task_id = ?1
            ORDER BY rowid
            ",
        )?;
        let rows = stmt.query_map([task_id], map_evidence)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists all evidence refs across tasks.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_all_evidence(&self) -> StoreResult<Vec<EvidenceRef>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT evidence_id, task_id, source_kind, source_ref, label, summary, related_handoff_id,
                   related_session_id, related_memory_query, related_symbol, related_file, schema_version
            FROM evidence_refs
            ORDER BY rowid
            ",
        )?;
        let rows = stmt.query_map([], map_evidence)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists evidence references for all tasks in a project.
    ///
    /// When `project_root` is `None`, all evidence is returned (equivalent to
    /// [`list_all_evidence`](Self::list_all_evidence)).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_evidence_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<EvidenceRef>> {
        if let Some(project_root) = project_root {
            let mut stmt = self.conn.prepare(
                r"
                SELECT e.evidence_id, e.task_id, e.source_kind, e.source_ref, e.label, e.summary, e.related_handoff_id,
                       e.related_session_id, e.related_memory_query, e.related_symbol, e.related_file, e.schema_version
                FROM evidence_refs e
                JOIN tasks t ON t.task_id = e.task_id
                WHERE t.project_root = ?1
                ORDER BY e.rowid
                ",
            )?;
            let rows = stmt.query_map([project_root], map_evidence)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::from)
        } else {
            self.list_all_evidence()
        }
    }
}
