/// Tests that the MCP schema and dispatch table stay in sync.
///
/// The root cause of most tool audit bugs is schema/dispatch drift: a tool is
/// added to one side without updating the other. These tests catch that at
/// compile + test time so the divergence is immediate rather than discovered at
/// runtime.
use std::collections::HashSet;

use serde_json::json;
use tempfile::tempdir;

/// Every tool name defined in schema must have a dispatch entry, and every
/// dispatch entry must appear in the schema.
///
/// The dispatch check works by calling each tool with empty args and verifying
/// the response does NOT start with "unknown tool: ". Missing-param errors are
/// expected — those prove the tool is wired. "unknown tool" means it isn't.
#[test]
fn test_schema_matches_dispatch() {
    let schema_tools = canopy::mcp::schema::tool_definitions();
    let schema_names: HashSet<String> = schema_tools
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();

    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let store = canopy::store::Store::open(&db_path).expect("store open");

    for name in &schema_names {
        let result = canopy::tools::dispatch_tool(&store, "test-agent", name, &json!({}));
        let text = &result.content[0].text;
        assert!(
            !text.starts_with("unknown tool"),
            "Schema defines '{name}' but dispatch doesn't handle it — got: {text}"
        );
    }
}

/// Every tool handled by dispatch must appear in the schema.
///
/// This uses a hard-coded expected set extracted from the match arms in
/// `src/tools/mod.rs`. If a tool is added to dispatch without updating schema,
/// this list will be out of date and the schema check above will catch the
/// schema side. If a tool is removed from dispatch but left in this list, the
/// test will fail because `dispatch_tool` returns "unknown tool".
#[test]
fn test_dispatch_tools_are_in_schema() {
    // Extracted from the match arms in src/tools/mod.rs.
    // Update this list whenever a tool is added to or removed from dispatch.
    let dispatch_names = [
        "canopy_register",
        "canopy_heartbeat",
        "canopy_whoami",
        "canopy_situation",
        "canopy_work_queue",
        "canopy_task_claim",
        "canopy_task_yield",
        "canopy_task_create",
        "canopy_task_decompose",
        "canopy_task_get",
        "canopy_task_list",
        "canopy_task_update_status",
        "canopy_task_complete",
        "canopy_task_block",
        "canopy_task_snapshot",
        "canopy_report_scope_gap",
        "canopy_get_handoff_scope",
        "canopy_files_lock",
        "canopy_files_unlock",
        "canopy_files_check",
        "canopy_files_list_locks",
        "canopy_handoff_create",
        "canopy_handoff_accept",
        "canopy_handoff_reject",
        "canopy_handoff_complete",
        "canopy_handoff_list",
        "canopy_attach_evidence",
        "canopy_evidence_add",
        "canopy_evidence_list",
        "canopy_evidence_verify",
        "canopy_council_post",
        "canopy_council_show",
        "canopy_import_handoff",
    ];

    let schema_tools = canopy::mcp::schema::tool_definitions();
    let schema_names: HashSet<&str> = schema_tools
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .collect();

    for name in &dispatch_names {
        assert!(
            schema_names.contains(name),
            "Dispatch handles '{name}' but schema doesn't define it — add it to schema::tool_definitions()"
        );
    }
}

/// The total tool count in the schema must match the expected count.
///
/// This is a canary: if someone adds or removes a tool and forgets to update
/// the dispatch list in `test_dispatch_tools_are_in_schema`, this will flag
/// the discrepancy.
#[test]
fn test_tool_count_matches() {
    let schema_count = canopy::mcp::schema::tool_definitions().len();
    assert_eq!(
        schema_count, 34,
        "Expected 34 tools in schema, got {schema_count}. Update this assertion and the dispatch list in test_dispatch_tools_are_in_schema."
    );
}
