//! Tools for tool adoption scoring.
//!
//! Provides one MCP tool:
//! - `canopy_record_tool_usage` — parse and store a `tool-usage-event-v1` JSON blob

use crate::models::ToolAdoptionScore;
use crate::store::CanopyStore;
use crate::tools::ToolResult;
use serde_json::{Value, json};

/// Represents a tool entry from the tool-usage-event-v1 payload.
#[derive(Debug, Clone)]
struct ToolEntry {
    tool_name: String,
    source: String,
}

/// `canopy_record_tool_usage`: parse and store a `tool-usage-event-v1` JSON blob.
///
/// Accepts a `json` string argument containing the full payload.
/// If the payload includes a `task_id`, computes the adoption score and links it to that task.
pub fn tool_record_tool_usage(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    // Extract the JSON payload
    let raw: Value = if let Some(s) = args.get("json").and_then(Value::as_str) {
        match serde_json::from_str(s) {
            Ok(obj) => obj,
            Err(e) => {
                return ToolResult::error(format!("failed to parse 'json' parameter as JSON: {e}"));
            }
        }
    } else if let Some(obj) = args.get("json_object") {
        obj.clone()
    } else {
        return ToolResult::error(
            "missing required parameter: provide 'json' (string) or 'json_object' (object)"
                .to_string(),
        );
    };

    // Validate schema version
    if raw.get("schema_version").and_then(Value::as_str) != Some("1.0") {
        return ToolResult::error(
            "expected schema_version: '1.0' for tool-usage-event-v1".to_string(),
        );
    }

    // Extract optional task_id
    let Some(task_id) = raw.get("task_id").and_then(Value::as_str) else {
        return ToolResult::text(
            "tool usage event recorded (no task_id provided, adoption score not computed)"
                .to_string(),
        );
    };

    // Extract the three tool arrays
    let tools_available = extract_tool_entries(&raw, "tools_available");
    let tools_called = extract_tool_entries(&raw, "tools_called");
    let tools_relevant_unused = extract_tool_entries(&raw, "tools_relevant_unused");

    // Build tuples of (name, source) for the compute method
    let available_tuples: Vec<(&str, &str)> = tools_available
        .iter()
        .map(|t| (t.tool_name.as_str(), t.source.as_str()))
        .collect();

    let called_tuples: Vec<(&str, &str)> = tools_called
        .iter()
        .map(|t| (t.tool_name.as_str(), t.source.as_str()))
        .collect();

    let relevant_tuples: Vec<(&str, &str)> = tools_relevant_unused
        .iter()
        .map(|t| (t.tool_name.as_str(), t.source.as_str()))
        .collect();

    // Combine called and relevant_unused to get all relevant tools
    let mut all_relevant = relevant_tuples;
    all_relevant.extend(called_tuples.iter().copied());

    // Compute the adoption score
    let adoption_score =
        ToolAdoptionScore::compute(&called_tuples, &all_relevant, &available_tuples);

    // Store it
    match store.record_tool_adoption_score(task_id, &adoption_score) {
        Ok(()) => ToolResult::json(&json!({
            "message": "tool adoption score computed and stored",
            "task_id": task_id,
            "score": adoption_score.score,
            "tools_used": adoption_score.tools_used,
            "tools_relevant": adoption_score.tools_relevant,
            "tools_available": adoption_score.tools_available,
        })),
        Err(e) => ToolResult::error(format!(
            "failed to store tool adoption score for task {task_id}: {e}"
        )),
    }
}

/// Extract tool entries from an array in the event payload.
fn extract_tool_entries(event: &Value, key: &str) -> Vec<ToolEntry> {
    event
        .get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let tool_name = item.get("tool_name").and_then(Value::as_str)?;
                    let source = item.get("source").and_then(Value::as_str)?;
                    Some(ToolEntry {
                        tool_name: tool_name.to_string(),
                        source: source.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
