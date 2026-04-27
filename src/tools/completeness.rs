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
/// and optional verify script results. The verify script is only executed
/// when the caller explicitly sets `run_verify_script: true`; omitting the
/// flag or setting it to `false` leaves `verify_passed` and `verify_output`
/// as `null` in the response.
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

    // Only execute the verify script when the caller explicitly opts in.
    // Defaulting to false prevents MCP callers from triggering shell
    // execution on the server by pointing at a handoff that happens to
    // have a paired verify script.
    let caller_wants_verify = args
        .get("run_verify_script")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let (verify_passed, verify_output) = if caller_wants_verify && report.has_verify_script {
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

#[cfg(test)]
mod tests {
    use super::tool_check_handoff_completeness;
    use crate::store::Store;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn completeness_does_not_run_verify_script_without_explicit_flag() {
        let dir = tempdir().expect("tempdir");
        let handoff_path = dir.path().join("demo.md");
        fs::write(&handoff_path, "# Handoff: Demo\n\n- [x] Step done\n")
            .expect("write handoff");

        // Create a verify script that would produce a detectable sentinel file
        // if executed, so the test can confirm it was NOT run.
        let verify_path = dir.path().join("verify-demo.sh");
        let sentinel = dir.path().join("script_was_run");
        fs::write(
            &verify_path,
            format!(
                "#!/bin/sh\ntouch '{}'\necho 'Results: 1 passed, 0 failed'\n",
                sentinel.display()
            ),
        )
        .expect("write verify script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&verify_path, fs::Permissions::from_mode(0o755))
                .expect("chmod verify script");
        }

        let db_path = dir.path().join("canopy.db");
        let store = Store::open(&db_path).expect("open store");

        // Call without the run_verify_script flag — script must not execute.
        let result = tool_check_handoff_completeness(
            &store,
            "agent-1",
            &json!({ "handoff_path": handoff_path.display().to_string() }),
        );
        assert!(!result.is_error, "unexpected error: {:?}", result.content);

        let payload: serde_json::Value =
            serde_json::from_str(&result.content[0].text).expect("json result");
        assert!(
            payload["verify_passed"].is_null(),
            "verify_passed should be null when flag is absent"
        );
        assert!(
            payload["verify_output"].is_null(),
            "verify_output should be null when flag is absent"
        );
        assert!(
            !sentinel.exists(),
            "verify script must not have executed without the explicit allow flag"
        );
    }

    #[test]
    fn completeness_runs_verify_script_when_flag_is_true() {
        let dir = tempdir().expect("tempdir");
        let handoff_path = dir.path().join("demo.md");
        fs::write(&handoff_path, "# Handoff: Demo\n\n- [x] Step done\n")
            .expect("write handoff");

        let verify_path = dir.path().join("verify-demo.sh");
        fs::write(
            &verify_path,
            "#!/bin/sh\necho 'Results: 2 passed, 0 failed'\n",
        )
        .expect("write verify script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&verify_path, fs::Permissions::from_mode(0o755))
                .expect("chmod verify script");
        }

        let db_path = dir.path().join("canopy.db");
        let store = Store::open(&db_path).expect("open store");

        let result = tool_check_handoff_completeness(
            &store,
            "agent-1",
            &json!({
                "handoff_path": handoff_path.display().to_string(),
                "run_verify_script": true
            }),
        );
        assert!(!result.is_error, "unexpected error: {:?}", result.content);

        let payload: serde_json::Value =
            serde_json::from_str(&result.content[0].text).expect("json result");
        assert_eq!(
            payload["verify_passed"],
            true,
            "verify_passed should be true when script succeeds"
        );
        assert!(
            payload["verify_output"].as_str().is_some(),
            "verify_output should be populated"
        );
    }
}
