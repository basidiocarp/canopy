// Tools exported from this module:
// - tool_attach_evidence
// - tool_evidence_add
// - tool_evidence_list
// - tool_evidence_verify

use crate::models::{EvidenceRef, EvidenceSourceKind};
use crate::store::{CanopyStore, EvidenceLinkRefs};
use crate::tools::{ToolResult, get_str, validate_required_string};
use serde::Serialize;
use serde_json::Value;
use std::str::FromStr;
use ulid::Ulid;

#[derive(Debug, Serialize)]
struct TaskEvidenceSummary {
    task_id: String,
    evidence_count: usize,
    evidence: Vec<EvidenceRef>,
}

/// Attach evidence using the agent-facing compact MCP form.
pub fn tool_attach_evidence(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let evidence_type_str = match validate_required_string(args, "evidence_type") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Ok(evidence_type) = EvidenceSourceKind::from_str(evidence_type_str) else {
        return ToolResult::error(format!("invalid evidence_type: {evidence_type_str}"));
    };
    let ref_id = match validate_required_string(args, "ref_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(source_ref) = validate_evidence_ref(evidence_type, ref_id) else {
        return ToolResult::error(format!("invalid ref_id for {evidence_type_str}: {ref_id}"));
    };
    let summary = get_str(args, "note");
    let label = evidence_label(evidence_type);

    if let Err(error) = store.add_evidence(
        task_id,
        evidence_type,
        source_ref,
        &label,
        summary,
        EvidenceLinkRefs::default(),
    ) {
        return ToolResult::error(format!("failed to add evidence: {error}"));
    }

    match store.list_evidence(task_id) {
        Ok(evidence) => ToolResult::json(&TaskEvidenceSummary {
            task_id: task_id.to_string(),
            evidence_count: evidence.len(),
            evidence,
        }),
        Err(error) => ToolResult::error(format!("failed to list evidence: {error}")),
    }
}

/// Attach evidence to a task using the legacy, fully-explicit MCP shape.
pub fn tool_evidence_add(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let source_kind_str = match validate_required_string(args, "source_kind") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Ok(source_kind) = EvidenceSourceKind::from_str(source_kind_str) else {
        return ToolResult::error(format!("invalid source_kind: {source_kind_str}"));
    };
    let source_ref = match validate_required_string(args, "source_ref") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(source_ref) = validate_evidence_ref(source_kind, source_ref) else {
        return ToolResult::error(format!(
            "invalid source_ref for {source_kind_str}: {source_ref}"
        ));
    };
    let label = match validate_required_string(args, "label") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let summary = get_str(args, "summary");
    let links = EvidenceLinkRefs {
        related_handoff_id: get_str(args, "related_handoff_id"),
        session_id: get_str(args, "related_session_id"),
        file: get_str(args, "related_file"),
        ..EvidenceLinkRefs::default()
    };

    match store.add_evidence(task_id, source_kind, source_ref, label, summary, links) {
        Ok(evidence) => ToolResult::json(&evidence),
        Err(error) => ToolResult::error(format!("failed to add evidence: {error}")),
    }
}

fn evidence_label(source_kind: EvidenceSourceKind) -> String {
    let label = source_kind.to_string().replace('_', " ");
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => {
            let mut head = first.to_uppercase().collect::<String>();
            head.push_str(chars.as_str());
            head
        }
        None => label,
    }
}

fn validate_evidence_ref(source_kind: EvidenceSourceKind, source_ref: &str) -> Option<&str> {
    let trimmed = source_ref.trim();
    if trimmed.is_empty() {
        return None;
    }

    let valid = match source_kind {
        EvidenceSourceKind::HyphaeSession => {
            looks_like_hyphae_ref(trimmed, &["session:", "ses_"]) || looks_like_ulid(trimmed)
        }
        EvidenceSourceKind::HyphaeRecall => {
            looks_like_hyphae_ref(trimmed, &["rec_", "recall:"]) || looks_like_ulid(trimmed)
        }
        EvidenceSourceKind::HyphaeOutcome => {
            looks_like_hyphae_ref(trimmed, &["sig_", "outcome:"]) || looks_like_ulid(trimmed)
        }
        EvidenceSourceKind::CortinaEvent => trimmed.starts_with("cortina://"),
        EvidenceSourceKind::MyceliumCommand
        | EvidenceSourceKind::MyceliumExplain
        | EvidenceSourceKind::RhizomeImpact
        | EvidenceSourceKind::RhizomeExport
        | EvidenceSourceKind::ScriptVerification
        | EvidenceSourceKind::ManualNote => true,
    };

    valid.then_some(trimmed)
}

fn looks_like_hyphae_ref(value: &str, prefixes: &[&str]) -> bool {
    prefixes
        .iter()
        .any(|prefix| value.strip_prefix(prefix).is_some_and(looks_like_ulid))
}

fn looks_like_ulid(value: &str) -> bool {
    value.parse::<Ulid>().is_ok()
}

/// List evidence for a task.
pub fn tool_evidence_list(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match store.list_evidence(task_id) {
        Ok(evidence) => ToolResult::json(&evidence),
        Err(error) => ToolResult::error(format!("failed to list evidence: {error}")),
    }
}

#[derive(Debug, Serialize)]
struct EvidenceVerificationReport {
    verified: u32,
    failed: u32,
    unsupported: u32,
}

/// Verify evidence references.
pub fn tool_evidence_verify(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = get_str(args, "task_id");

    let evidence = match task_id {
        Some(id) => match store.list_evidence(id) {
            Ok(e) => e,
            Err(error) => return ToolResult::error(format!("failed to list evidence: {error}")),
        },
        None => match store.list_all_evidence() {
            Ok(e) => e,
            Err(error) => return ToolResult::error(format!("failed to list evidence: {error}")),
        },
    };

    // All source kinds currently return unsupported — verification requires
    // live tool probes that are not available in this context.
    let report = EvidenceVerificationReport {
        verified: 0,
        failed: 0,
        unsupported: u32::try_from(evidence.len()).unwrap_or(u32::MAX),
    };

    ToolResult::json(&report)
}

#[cfg(test)]
mod tests {
    use super::{evidence_label, validate_evidence_ref};
    use crate::models::EvidenceSourceKind;
    use crate::store::Store;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn evidence_label_formats_human_readable_names() {
        assert_eq!(
            evidence_label(EvidenceSourceKind::HyphaeSession),
            "Hyphae session"
        );
        assert_eq!(
            evidence_label(EvidenceSourceKind::CortinaEvent),
            "Cortina event"
        );
    }

    #[test]
    fn validate_evidence_ref_accepts_expected_shapes() {
        assert_eq!(
            validate_evidence_ref(
                EvidenceSourceKind::HyphaeSession,
                "ses_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            ),
            Some("ses_01ARZ3NDEKTSV4RRFFQ69G5FAV")
        );
        assert_eq!(
            validate_evidence_ref(
                EvidenceSourceKind::HyphaeSession,
                "session:01ARZ3NDEKTSV4RRFFQ69G5FAV"
            ),
            Some("session:01ARZ3NDEKTSV4RRFFQ69G5FAV")
        );
        assert_eq!(
            validate_evidence_ref(
                EvidenceSourceKind::CortinaEvent,
                "cortina://outcome/error_detected/ses-1/123"
            ),
            Some("cortina://outcome/error_detected/ses-1/123")
        );
        assert_eq!(
            validate_evidence_ref(EvidenceSourceKind::ManualNote, "note-1"),
            Some("note-1")
        );
        assert_eq!(
            validate_evidence_ref(EvidenceSourceKind::HyphaeOutcome, "not-a-valid-ref"),
            None
        );
    }

    #[test]
    fn tool_attach_evidence_returns_updated_task_summary() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("canopy.db");
        let store = Store::open(&db_path).expect("store open");
        let task = store
            .create_task("Attach evidence", None, "operator", "/tmp/project", None)
            .expect("create task");
        let task_id = task.task_id.clone();

        let result = super::tool_attach_evidence(
            &store,
            "agent-1",
            &json!({
                "task_id": task_id,
                "evidence_type": "hyphae_session",
                "ref_id": "session:01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "note": "Mid-task session evidence",
            }),
        );

        assert!(
            !result.is_error,
            "unexpected error result: {:?}",
            result.content
        );
        let payload: serde_json::Value =
            serde_json::from_str(&result.content[0].text).expect("json summary");
        assert_eq!(payload["task_id"], task_id);
        assert_eq!(payload["evidence_count"], 1);
        assert_eq!(payload["evidence"][0]["source_kind"], "hyphae_session");
        assert_eq!(
            payload["evidence"][0]["source_ref"],
            "session:01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
    }
}
