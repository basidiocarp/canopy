//! Machine-checkable tests that verify Canopy's Rust types stay aligned
//! with the ecosystem contract schemas in `../septa/*.schema.json`.
//!
//! These tests prevent the class of bug where a Rust enum gets a new variant
//! but the corresponding contract schema is not updated (or vice versa).

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

fn contracts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("canopy should be inside basidiocarp workspace")
        .join("contracts")
}

fn septa_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("canopy should be inside basidiocarp workspace")
        .join("septa")
}

/// Extract enum values from a JSON Schema's `source_kind.enum` array.
fn extract_schema_enum(schema_json: &serde_json::Value, field: &str) -> HashSet<String> {
    schema_json
        .get("properties")
        .and_then(|p| p.get(field))
        .and_then(|f| f.get("enum"))
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Get Rust enum variants as `snake_case` strings (matching serde serialization).
fn rust_evidence_source_kinds() -> HashSet<String> {
    // These must match the variants in models.rs EvidenceSourceKind
    // with #[serde(rename_all = "snake_case")]
    [
        "hyphae_session",
        "hyphae_recall",
        "hyphae_outcome",
        "cortina_event",
        "mycelium_command",
        "mycelium_explain",
        "rhizome_impact",
        "rhizome_export",
        "script_verification",
        "manual_note",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn rust_handoff_statuses() -> HashSet<String> {
    [
        "open",
        "accepted",
        "rejected",
        "expired",
        "cancelled",
        "completed",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

#[test]
fn evidence_source_kinds_match_contract() {
    let schema_path = contracts_dir().join("evidence-ref-v1.schema.json");
    if !schema_path.exists() {
        eprintln!(
            "Skipping: contracts dir not found at {}",
            schema_path.display()
        );
        return;
    }

    let schema_text = fs::read_to_string(&schema_path).expect("read evidence-ref schema");
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("parse evidence-ref schema");

    let schema_kinds = extract_schema_enum(&schema, "source_kind");
    let rust_kinds = rust_evidence_source_kinds();

    let in_rust_not_schema: Vec<_> = rust_kinds.difference(&schema_kinds).collect();
    let in_schema_not_rust: Vec<_> = schema_kinds.difference(&rust_kinds).collect();

    assert!(
        in_rust_not_schema.is_empty(),
        "EvidenceSourceKind variants in Rust but missing from evidence-ref-v1.schema.json: {in_rust_not_schema:?}. \
         Update the contract schema to include these."
    );

    assert!(
        in_schema_not_rust.is_empty(),
        "Evidence source kinds in contract schema but missing from Rust EvidenceSourceKind: {in_schema_not_rust:?}. \
         Add these variants to models.rs or remove from the schema."
    );
}

#[test]
fn snapshot_schema_required_fields_present() {
    let schema_path = contracts_dir().join("canopy-snapshot-v1.schema.json");
    if !schema_path.exists() {
        eprintln!("Skipping: canopy-snapshot-v1.schema.json not found");
        return;
    }

    let schema_text = fs::read_to_string(&schema_path).expect("read snapshot schema");
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("parse snapshot schema");

    let required = schema
        .get("required")
        .and_then(|r| r.as_array())
        .expect("snapshot schema should have required array");

    let required_fields: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();

    // These are the fields the contract guarantees.
    // If api.rs changes the snapshot shape, this test catches it.
    for field in &[
        "schema_version",
        "attention",
        "sla_summary",
        "tasks",
        "evidence",
    ] {
        assert!(
            required_fields.contains(field),
            "canopy-snapshot-v1 schema missing required field '{field}'"
        );
    }
}

#[test]
fn task_detail_schema_required_fields_present() {
    let schema_path = contracts_dir().join("canopy-task-detail-v1.schema.json");
    if !schema_path.exists() {
        eprintln!("Skipping: canopy-task-detail-v1.schema.json not found");
        return;
    }

    let schema_text = fs::read_to_string(&schema_path).expect("read task-detail schema");
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("parse task-detail schema");

    let required = schema
        .get("required")
        .and_then(|r| r.as_array())
        .expect("task-detail schema should have required array");

    let required_fields: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();

    for field in &[
        "schema_version",
        "task",
        "attention",
        "sla_summary",
        "allowed_actions",
        "evidence",
    ] {
        assert!(
            required_fields.contains(field),
            "canopy-task-detail-v1 schema missing required field '{field}'"
        );
    }
}

#[test]
fn workflow_participant_runtime_identity_contract_has_core_fields() {
    let schema_path = septa_dir().join("workflow-participant-runtime-identity-v1.schema.json");
    if !schema_path.exists() {
        eprintln!("Skipping: workflow-participant-runtime-identity-v1.schema.json not found");
        return;
    }

    let schema_text = fs::read_to_string(&schema_path).expect("read workflow identity schema");
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("parse workflow identity schema");

    let required = schema
        .get("required")
        .and_then(|r| r.as_array())
        .expect("workflow identity schema should have required array");
    let required_fields: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();

    for field in &["schema_version", "workflow_id", "participant_id"] {
        assert!(
            required_fields.contains(field),
            "workflow-participant-runtime-identity-v1 schema missing required field '{field}'"
        );
    }

    let properties = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("workflow identity schema should have properties");

    for field in &[
        "runtime_session_id",
        "project_root",
        "worktree_id",
        "host_ref",
        "backend_ref",
    ] {
        assert!(
            properties.contains_key(*field),
            "workflow-participant-runtime-identity-v1 schema missing property '{field}'"
        );
    }
}

#[test]
fn handoff_statuses_cover_all_variants() {
    // Verify our known handoff statuses match what the MCP schema advertises.
    // The handoff-context schema uses stop_reason (not handoff status),
    // but the MCP tool schema for canopy_handoff_list uses status filtering.
    let expected = rust_handoff_statuses();

    // Verify the MCP schema's canopy_handoff_list has matching status values
    let tools = canopy::mcp::schema::tool_definitions();
    let handoff_list_tool = tools
        .iter()
        .find(|t| t.get("name").and_then(|n| n.as_str()) == Some("canopy_handoff_list"))
        .expect("canopy_handoff_list should be in schema");

    let schema_statuses: HashSet<String> = handoff_list_tool
        .pointer("/inputSchema/properties/status/enum")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if !schema_statuses.is_empty() {
        let in_rust_not_schema: Vec<_> = expected.difference(&schema_statuses).collect();
        assert!(
            in_rust_not_schema.is_empty(),
            "HandoffStatus variants in Rust but missing from canopy_handoff_list schema: {in_rust_not_schema:?}"
        );
    }
}

#[test]
fn handoff_context_stop_reasons_are_documented() {
    let schema_path = contracts_dir().join("handoff-context-v1.schema.json");
    if !schema_path.exists() {
        eprintln!("Skipping: handoff-context-v1.schema.json not found");
        return;
    }

    let schema_text = fs::read_to_string(&schema_path).expect("read handoff-context schema");
    let schema: serde_json::Value =
        serde_json::from_str(&schema_text).expect("parse handoff-context schema");

    // Verify the boundary.stop_reason enum exists and has expected values
    let stop_reasons = schema
        .get("properties")
        .and_then(|p| p.get("boundary"))
        .and_then(|b| b.get("properties"))
        .and_then(|p| p.get("stop_reason"))
        .and_then(|sr| sr.get("enum"))
        .and_then(|e| e.as_array())
        .expect("handoff-context should define boundary.stop_reason enum");

    let reasons: Vec<&str> = stop_reasons.iter().filter_map(|v| v.as_str()).collect();

    // These are the stop reasons agents can use in handoffs.
    // If new reasons are added to the schema, tools must handle them.
    assert!(
        reasons.contains(&"completed"),
        "handoff-context missing 'completed' stop reason"
    );
    assert!(
        reasons.contains(&"blocked"),
        "handoff-context missing 'blocked' stop reason"
    );
    assert!(
        reasons.contains(&"needs_review"),
        "handoff-context missing 'needs_review' stop reason"
    );
}

#[test]
fn mcp_tool_schemas_match_dispatch() {
    // Verify every tool in the MCP schema has a dispatch entry,
    // and every dispatch entry has a schema.
    // This is a stronger version of the test in schema_drift.rs
    // that also checks the tool count hasn't changed unexpectedly.
    let schema_tools = canopy::mcp::schema::tool_definitions();
    let schema_names: HashSet<String> = schema_tools
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();

    assert_eq!(
        schema_names.len(),
        38,
        "Expected 38 MCP tools, got {}. If you added/removed tools, update this test.",
        schema_names.len()
    );

    // Verify dispatch handles every schema tool
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let store = canopy::store::Store::open(&db_path).unwrap();

    for name in &schema_names {
        let result =
            canopy::tools::dispatch_tool(&store, "test-agent", name, &serde_json::json!({}));
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(
            !serialized.contains("unknown tool"),
            "MCP schema defines '{name}' but dispatch returns 'unknown tool'"
        );
    }
}
