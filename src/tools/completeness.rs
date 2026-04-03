// Tools exported from this module:
// - tool_check_handoff_completeness

use crate::handoff_check::{check_completeness, run_verify_script};
use crate::store::CanopyStore;
use crate::tools::{ToolResult, validate_required_string};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Serialize)]
struct CompletenessCheckResult {
    is_complete: bool,
    total_checkboxes: usize,
    checked_checkboxes: usize,
    empty_paste_markers: Vec<usize>,
    has_verify_script: bool,
    verify_passed: Option<bool>,
    verify_output: Option<String>,
    summary: String,
}

/// Check whether a handoff document meets completion criteria.
///
/// Returns a structured report with checkbox counts, paste marker status,
/// and optional verify script results.
pub fn tool_check_handoff_completeness(
    _store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let handoff_path_str = match validate_required_string(args, "handoff_path") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let handoff_path = Path::new(handoff_path_str);

    let report = match check_completeness(handoff_path) {
        Ok(r) => r,
        Err(e) => return ToolResult::error(format!("failed to check completeness: {e}")),
    };

    let summary = crate::handoff_check::format_incomplete_report(&report);

    // Optionally run verify script
    let (verify_passed, verify_output) = if report.has_verify_script {
        match run_verify_script(&report) {
            Ok(result) => (Some(result.success), Some(result.output)),
            Err(e) => (Some(false), Some(format!("verify script error: {e}"))),
        }
    } else {
        (None, None)
    };

    let result = CompletenessCheckResult {
        is_complete: report.is_complete,
        total_checkboxes: report.total_checkboxes,
        checked_checkboxes: report.checked_checkboxes,
        empty_paste_markers: report.empty_paste_markers,
        has_verify_script: report.has_verify_script,
        verify_passed,
        verify_output,
        summary,
    };

    ToolResult::json(&result)
}
