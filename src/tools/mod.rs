pub mod completeness;
pub mod council;
pub mod evidence;
pub mod files;
pub mod handoff;
pub mod identity;
pub mod import;
pub mod outcomes;
pub mod policy;
pub mod queue;
pub mod scope;
pub mod task;

use serde::Serialize;
use serde_json::Value;

use crate::runtime::DispatchDecision;
use crate::store::CanopyStore;

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Serialize)]
pub struct ToolResult {
    pub content: Vec<TextContent>,
    #[serde(rename = "isError", skip_serializing_if = "is_false")]
    pub is_error: bool,
}

#[derive(Debug, Serialize)]
pub struct TextContent {
    #[serde(rename = "type")]
    pub content_type: &'static str,
    pub text: String,
}

impl ToolResult {
    /// Create a successful text result.
    #[must_use]
    pub fn text(text: String) -> Self {
        Self {
            content: vec![TextContent {
                content_type: "text",
                text,
            }],
            is_error: false,
        }
    }

    /// Create an error result.
    #[must_use]
    pub fn error(text: String) -> Self {
        Self {
            content: vec![TextContent {
                content_type: "text",
                text,
            }],
            is_error: true,
        }
    }

    /// Create a result by serializing a value to JSON.
    #[must_use]
    pub fn json<T: Serialize>(value: &T) -> Self {
        match serde_json::to_string_pretty(value) {
            Ok(text) => Self::text(text),
            Err(error) => Self::error(format!("serialization error: {error}")),
        }
    }
}

/// Extract an optional string from a JSON object by key.
#[must_use]
pub fn get_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Extract a required string parameter, returning a `ToolResult` error on failure.
///
/// # Errors
///
/// Returns a `ToolResult` error if the key is missing or not a string.
pub fn validate_required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolResult> {
    get_str(args, key)
        .ok_or_else(|| ToolResult::error(format!("missing required parameter: {key}")))
}

/// Extract an integer parameter with bounds clamping.
#[must_use]
pub fn get_bounded_i64(args: &Value, key: &str, default: i64, min: i64, max: i64) -> i64 {
    args.get(key)
        .and_then(Value::as_i64)
        .unwrap_or(default)
        .clamp(min, max)
}

/// Extract a string array from a JSON object by key.
#[must_use]
pub fn get_string_array(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Dispatch a tool call to the appropriate handler.
///
/// Before routing to the handler, the active [`policy::DispatchPolicy`] is
/// evaluated against the tool's annotations.  A [`DispatchDecision::FlagForReview`]
/// result causes an error [`ToolResult`] to be returned immediately so the
/// MCP server can surface the block to the operator.
#[must_use]
pub fn dispatch_tool(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    name: &str,
    args: &Value,
) -> ToolResult {
    // Policy check: look up the tool's annotations and evaluate the active policy.
    let annotations = policy::annotations_for_tool(name);
    if let DispatchDecision::FlagForReview { reason } =
        policy::DispatchPolicy::Default.evaluate(name, annotations)
    {
        return ToolResult::error(format!("policy blocked: {reason}"));
    }

    match name {
        "canopy_register" => identity::tool_register(store, agent_id, args),
        "canopy_heartbeat" => identity::tool_heartbeat(store, agent_id, args),
        "canopy_whoami" => identity::tool_whoami(store, agent_id, args),
        "canopy_situation" => identity::tool_situation(store, agent_id, args),
        "canopy_work_queue" => queue::tool_work_queue(store, agent_id, args),
        "canopy_task_claim" => queue::tool_task_claim(store, agent_id, args),
        "canopy_task_yield" => queue::tool_task_yield(store, agent_id, args),
        "canopy_files_lock" => files::tool_files_lock(store, agent_id, args),
        "canopy_files_unlock" => files::tool_files_unlock(store, agent_id, args),
        "canopy_files_check" => files::tool_files_check(store, agent_id, args),
        "canopy_files_list_locks" => files::tool_files_list_locks(store, agent_id, args),
        "canopy_task_create" => task::tool_task_create(store, agent_id, args),
        "canopy_task_decompose" => task::tool_task_decompose(store, agent_id, args),
        "canopy_task_get" => task::tool_task_get(store, agent_id, args),
        "canopy_task_list" => task::tool_task_list(store, agent_id, args),
        "canopy_task_update_status" => task::tool_task_update_status(store, agent_id, args),
        "canopy_task_complete" => task::tool_task_complete(store, agent_id, args),
        "canopy_task_block" => task::tool_task_block(store, agent_id, args),
        "canopy_task_snapshot" => task::tool_task_snapshot(store, agent_id, args),
        "canopy_report_scope_gap" => scope::tool_report_scope_gap(store, agent_id, args),
        "canopy_get_handoff_scope" => scope::tool_get_handoff_scope(store, agent_id, args),
        "canopy_handoff_create" => handoff::tool_handoff_create(store, agent_id, args),
        "canopy_handoff_accept" => handoff::tool_handoff_accept(store, agent_id, args),
        "canopy_handoff_reject" => handoff::tool_handoff_reject(store, agent_id, args),
        "canopy_handoff_complete" => handoff::tool_handoff_complete(store, agent_id, args),
        "canopy_handoff_list" => handoff::tool_handoff_list(store, agent_id, args),
        "canopy_attach_evidence" => evidence::tool_attach_evidence(store, agent_id, args),
        "canopy_evidence_add" => evidence::tool_evidence_add(store, agent_id, args),
        "canopy_evidence_list" => evidence::tool_evidence_list(store, agent_id, args),
        "canopy_evidence_verify" => evidence::tool_evidence_verify(store, agent_id, args),
        "canopy_council_post" => council::tool_council_post(store, agent_id, args),
        "canopy_council_show" => council::tool_council_show(store, agent_id, args),
        "canopy_import_handoff" => import::tool_import_handoff(store, agent_id, args),
        "canopy_check_handoff_completeness" => {
            completeness::tool_check_handoff_completeness(store, agent_id, args)
        }
        "canopy_outcome_record" => outcomes::tool_outcome_record(store, agent_id, args),
        "canopy_outcome_list" => outcomes::tool_outcome_list(store, agent_id, args),
        "canopy_outcome_show" => outcomes::tool_outcome_show(store, agent_id, args),
        "canopy_outcome_summary" => outcomes::tool_outcome_summary(store, agent_id, args),
        _ => ToolResult::error(format!("unknown tool: {name}")),
    }
}
