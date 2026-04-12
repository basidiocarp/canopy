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
    evidence: Vec<EvidenceReviewRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EvidenceReviewRow {
    #[serde(flatten)]
    pub(crate) evidence: EvidenceRef,
    pub(crate) source_kind_label: String,
    pub(crate) caused_by: Vec<EvidenceAttributionRef>,
    pub(crate) review_summary: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EvidenceAttributionRef {
    pub(crate) relation: &'static str,
    pub(crate) reference: String,
}

pub fn build_evidence_review_rows(evidence: &[EvidenceRef]) -> Vec<EvidenceReviewRow> {
    evidence
        .iter()
        .cloned()
        .map(build_evidence_review_row)
        .collect()
}

fn build_evidence_review_row(evidence: EvidenceRef) -> EvidenceReviewRow {
    let source_kind_label = evidence_label(evidence.source_kind);
    let caused_by = build_caused_by_refs(&evidence);
    let review_summary = build_review_summary(&evidence, &source_kind_label, &caused_by);

    EvidenceReviewRow {
        evidence,
        source_kind_label,
        caused_by,
        review_summary,
    }
}

fn build_caused_by_refs(evidence: &EvidenceRef) -> Vec<EvidenceAttributionRef> {
    let mut refs = Vec::new();
    push_attribution_ref(&mut refs, "handoff", evidence.related_handoff_id.as_deref());
    push_attribution_ref(&mut refs, "session", evidence.related_session_id.as_deref());
    push_attribution_ref(
        &mut refs,
        "memory_query",
        evidence.related_memory_query.as_deref(),
    );
    push_attribution_ref(&mut refs, "symbol", evidence.related_symbol.as_deref());
    push_attribution_ref(&mut refs, "file", evidence.related_file.as_deref());
    refs
}

fn push_attribution_ref(
    refs: &mut Vec<EvidenceAttributionRef>,
    relation: &'static str,
    value: Option<&str>,
) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        refs.push(EvidenceAttributionRef {
            relation,
            reference: value.to_string(),
        });
    }
}

fn build_review_summary(
    evidence: &EvidenceRef,
    source_kind_label: &str,
    caused_by: &[EvidenceAttributionRef],
) -> String {
    if caused_by.is_empty() {
        return format!(
            "{source_kind_label} evidence '{}' points to {} with no recorded causal links",
            evidence.label, evidence.source_ref
        );
    }

    let links = caused_by
        .iter()
        .map(|link| format!("{}={}", link.relation, link.reference))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "{source_kind_label} evidence '{}' points to {} and is caused_by {links}",
        evidence.label, evidence.source_ref
    )
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
            evidence: build_evidence_review_rows(&evidence),
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
        memory_query: get_str(args, "related_memory_query"),
        symbol: get_str(args, "related_symbol"),
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
        Ok(evidence) => ToolResult::json(&build_evidence_review_rows(&evidence)),
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
    use super::{build_evidence_review_rows, evidence_label, validate_evidence_ref};
    use crate::models::{EvidenceRef, EvidenceSourceKind};
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
        assert_eq!(payload["evidence"][0]["source_kind_label"], "Hyphae session");
        assert_eq!(
            payload["evidence"][0]["caused_by"],
            json!([{ "relation": "session", "reference": "session:01ARZ3NDEKTSV4RRFFQ69G5FAV" }])
        );
        assert_eq!(
            payload["evidence"][0]["review_summary"],
            "Hyphae session evidence 'Hyphae session' points to session:01ARZ3NDEKTSV4RRFFQ69G5FAV and is caused_by session=session:01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
    }

    #[test]
    fn tool_evidence_add_preserves_memory_query_and_symbol_links() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("canopy.db");
        let store = Store::open(&db_path).expect("store open");
        let task = store
            .create_task("Attach evidence", None, "operator", "/tmp/project", None)
            .expect("create task");
        let task_id = task.task_id.clone();

        let result = super::tool_evidence_add(
            &store,
            "agent-1",
            &json!({
                "task_id": task_id,
                "source_kind": "hyphae_session",
                "source_ref": "session:01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "label": "Implementation session",
                "summary": "Shows the successful fix",
                "related_session_id": "ses_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "related_memory_query": "operator follow-up",
                "related_symbol": "crate::module::apply_fix",
                "related_file": "src/module.rs"
            }),
        );

        assert!(
            !result.is_error,
            "unexpected error result: {:?}",
            result.content
        );
        let payload: EvidenceRef =
            serde_json::from_str(&result.content[0].text).expect("json evidence");
        assert_eq!(
            payload.related_memory_query.as_deref(),
            Some("operator follow-up")
        );
        assert_eq!(
            payload.related_symbol.as_deref(),
            Some("crate::module::apply_fix")
        );

        let rows = build_evidence_review_rows(&[payload]);
        assert_eq!(rows[0].caused_by.len(), 4);
        assert_eq!(rows[0].caused_by[0].relation, "session");
        assert_eq!(rows[0].caused_by[1].relation, "memory_query");
        assert_eq!(rows[0].caused_by[2].relation, "symbol");
        assert_eq!(rows[0].caused_by[3].relation, "file");
    }

    #[test]
    fn build_evidence_review_rows_include_causal_links() {
        let evidence = EvidenceRef {
            schema_version: "1.0".to_string(),
            evidence_id: "evidence-1".to_string(),
            task_id: "task-1".to_string(),
            source_kind: EvidenceSourceKind::HyphaeSession,
            source_ref: "session:01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            label: "Implementation session".to_string(),
            summary: Some("Shows the successful fix".to_string()),
            related_handoff_id: Some("handoff-1".to_string()),
            related_session_id: Some("ses_01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()),
            related_memory_query: None,
            related_symbol: Some("crate::module::apply_fix".to_string()),
            related_file: Some("src/module.rs".to_string()),
        };

        let rows = build_evidence_review_rows(&[evidence]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_kind_label, "Hyphae session");
        assert_eq!(rows[0].caused_by.len(), 4);
        assert_eq!(rows[0].caused_by[0].relation, "handoff");
        assert_eq!(rows[0].caused_by[0].reference, "handoff-1");
        assert_eq!(rows[0].caused_by[1].relation, "session");
        assert_eq!(
            rows[0].review_summary,
            "Hyphae session evidence 'Implementation session' points to session:01ARZ3NDEKTSV4RRFFQ69G5FAV and is caused_by handoff=handoff-1, session=ses_01ARZ3NDEKTSV4RRFFQ69G5FAV, symbol=crate::module::apply_fix, file=src/module.rs"
        );
    }

    #[test]
    fn tool_evidence_list_returns_review_row_json_shape() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("canopy.db");
        let store = Store::open(&db_path).expect("store open");
        let task = store
            .create_task("List evidence", None, "operator", "/tmp/project", None)
            .expect("create task");
        let task_id = task.task_id.clone();

        let add_result = super::tool_evidence_add(
            &store,
            "agent-1",
            &json!({
                "task_id": task_id,
                "source_kind": "hyphae_session",
                "source_ref": "session:01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "label": "Implementation session",
                "summary": "Shows the successful fix",
                "related_session_id": "ses_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "related_memory_query": "operator follow-up",
                "related_symbol": "crate::module::apply_fix",
                "related_file": "src/module.rs"
            }),
        );
        assert!(
            !add_result.is_error,
            "unexpected error result: {:?}",
            add_result.content
        );

        let list_result = super::tool_evidence_list(
            &store,
            "agent-1",
            &json!({
                "task_id": task_id,
            }),
        );
        assert!(
            !list_result.is_error,
            "unexpected error result: {:?}",
            list_result.content
        );

        let payload: serde_json::Value =
            serde_json::from_str(&list_result.content[0].text).expect("json evidence list");
        assert_eq!(payload.as_array().expect("evidence array").len(), 1);
        assert_eq!(payload[0]["source_kind"], "hyphae_session");
        assert_eq!(payload[0]["source_kind_label"], "Hyphae session");
        assert_eq!(payload[0]["caused_by"][0]["relation"], "session");
        assert_eq!(payload[0]["caused_by"][1]["relation"], "memory_query");
        assert_eq!(payload[0]["caused_by"][2]["relation"], "symbol");
        assert_eq!(payload[0]["caused_by"][3]["relation"], "file");
        assert_eq!(
            payload[0]["review_summary"],
            "Hyphae session evidence 'Implementation session' points to session:01ARZ3NDEKTSV4RRFFQ69G5FAV and is caused_by session=ses_01ARZ3NDEKTSV4RRFFQ69G5FAV, memory_query=operator follow-up, symbol=crate::module::apply_fix, file=src/module.rs"
        );
    }
}
