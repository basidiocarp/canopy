// Tools exported from this module:
// - tool_evidence_add
// - tool_evidence_list
// - tool_evidence_verify

use crate::models::EvidenceSourceKind;
use crate::store::{CanopyStore, EvidenceLinkRefs};
use crate::tools::{ToolResult, get_str, validate_required_string};
use serde::Serialize;
use serde_json::Value;
use std::str::FromStr;

/// Attach evidence to a task.
pub fn tool_evidence_add(store: &(impl CanopyStore + ?Sized), _agent_id: &str, args: &Value) -> ToolResult {
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
        Err(e) => ToolResult::error(format!("failed to add evidence: {e}")),
    }
}

/// List evidence for a task.
pub fn tool_evidence_list(store: &(impl CanopyStore + ?Sized), _agent_id: &str, args: &Value) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match store.list_evidence(task_id) {
        Ok(evidence) => ToolResult::json(&evidence),
        Err(e) => ToolResult::error(format!("failed to list evidence: {e}")),
    }
}

#[derive(Debug, Serialize)]
struct EvidenceVerificationReport {
    verified: u32,
    failed: u32,
    unsupported: u32,
}

/// Verify evidence references.
pub fn tool_evidence_verify(store: &(impl CanopyStore + ?Sized), _agent_id: &str, args: &Value) -> ToolResult {
    let task_id = get_str(args, "task_id");

    let evidence = match task_id {
        Some(id) => match store.list_evidence(id) {
            Ok(e) => e,
            Err(e) => return ToolResult::error(format!("failed to list evidence: {e}")),
        },
        None => match store.list_all_evidence() {
            Ok(e) => e,
            Err(e) => return ToolResult::error(format!("failed to list evidence: {e}")),
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
