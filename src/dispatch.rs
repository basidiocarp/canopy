//! Dispatch request intake.
//!
//! Accepts a `dispatch-request-v1` payload, validates its fields, creates a
//! Canopy task, and applies the requested priority via triage.  The endpoint
//! is intentionally thin: it maps the external contract to coordination state
//! and returns the created task ID.

use crate::models::{AgentRole, TaskPriority};
use crate::store::{Store, TaskCreationOptions, TaskTriageUpdate};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: &str = "1.0";

/// Capability identifier for the Canopy dispatch endpoint.
///
/// Hymenium resolves this name via the capability registry to locate Canopy's
/// dispatch intake service.  The CLI (`canopy dispatch submit`) remains the
/// human/operator surface; internal orchestration must prefer resolving this
/// capability ID through Spore rather than rebuilding CLI flag calls.
pub const CAPABILITY_ID: &str = "workflow.dispatch.v1";

/// Priority from the dispatch-request-v1 schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl From<DispatchPriority> for TaskPriority {
    fn from(p: DispatchPriority) -> Self {
        match p {
            DispatchPriority::Low => TaskPriority::Low,
            DispatchPriority::Medium => TaskPriority::Medium,
            DispatchPriority::High => TaskPriority::High,
            DispatchPriority::Critical => TaskPriority::Critical,
        }
    }
}

/// Agent tier override from the dispatch-request-v1 schema.
///
/// Maps to a Canopy required role hint for the created task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchAgentTier {
    Opus,
    Sonnet,
    Haiku,
}

impl From<DispatchAgentTier> for AgentRole {
    fn from(tier: DispatchAgentTier) -> Self {
        match tier {
            // Opus maps to orchestrator role; sonnet/haiku map to implementer.
            DispatchAgentTier::Opus => AgentRole::Orchestrator,
            DispatchAgentTier::Sonnet | DispatchAgentTier::Haiku => AgentRole::Implementer,
        }
    }
}

/// Deserialised `dispatch-request-v1` payload.
///
/// `handoff_path` and `project_root` carry file-system paths that are passed to
/// subagents by reference; callers must never substitute file contents for these
/// fields. Passing a path (not inline content) is the empirically-validated
/// pattern that avoids redundant tool-call fan-out — the subagent resolves the
/// file itself rather than receiving its bytes inline.
///
/// This invariant is enforced when the struct is built via [`read_request`];
/// constructing a `DispatchRequest` directly (e.g. in tests) bypasses validation.
#[derive(Debug, Deserialize)]
pub struct DispatchRequest {
    pub schema_version: String,
    pub handoff_path: String,
    pub workflow_template: String,
    pub project_root: String,
    pub target_repo: String,
    pub priority: DispatchPriority,
    pub depends_on: Vec<String>,
    pub agent_tier_override: Option<DispatchAgentTier>,
}

/// Result returned by [`intake`].
#[derive(Debug, Serialize)]
pub struct DispatchResponse {
    pub task_id: String,
    pub workflow_template: String,
    pub priority_applied: String,
}

/// Read a `dispatch-request-v1` payload from a file or stdin (`-`).
///
/// Subagents receive a `handoff_path` and resolve the file themselves rather than
/// receiving the file's content inline — this is the empirically-validated pattern
/// that avoids redundant tool-call fan-out. To keep that contract intact, the parsed
/// `handoff_path` and `project_root` are validated as plausible path strings
/// (non-empty, no embedded newline) before the request is returned, so file contents
/// accidentally substituted for a path are rejected at the boundary.
///
/// # Errors
///
/// Returns an error if the file cannot be read, the JSON is malformed, or
/// `handoff_path`/`project_root` is empty or contains a newline character.
pub fn read_request(path: &str) -> Result<DispatchRequest> {
    let raw = if path == "-" {
        std::io::read_to_string(std::io::stdin()).context("reading dispatch request from stdin")?
    } else {
        std::fs::read_to_string(path)
            .with_context(|| format!("reading dispatch request from {path}"))?
    };
    let request: DispatchRequest =
        serde_json::from_str(&raw).context("parsing dispatch-request-v1 JSON")?;
    validate_path_field("handoff_path", &request.handoff_path)?;
    validate_path_field("project_root", &request.project_root)?;
    Ok(request)
}

/// Reject a `dispatch-request-v1` path field that is empty or carries an embedded
/// newline — both signal that file contents were substituted for a file-system path.
fn validate_path_field(field: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{field} must be a file-system path, not file contents (was empty)");
    }
    if value.contains('\n') {
        bail!(
            "{field} must be a file-system path, not file contents (contains an embedded newline)"
        );
    }
    Ok(())
}

/// Accept a `dispatch-request-v1` payload, create a Canopy task, and apply
/// the requested priority.
///
/// The `requested_by` value identifies the caller (e.g. `"hymenium"` or an
/// operator identifier).
///
/// # Errors
///
/// Returns an error if `schema_version` is not `"1.0"`, task creation fails,
/// or the triage update fails.
pub fn intake(
    store: &Store,
    request: &DispatchRequest,
    requested_by: &str,
) -> Result<DispatchResponse> {
    if request.schema_version != SCHEMA_VERSION {
        bail!(
            "unsupported dispatch-request schema version: {} (expected {SCHEMA_VERSION})",
            request.schema_version
        );
    }

    let title = format!("[{}] {}", request.workflow_template, request.handoff_path);
    let description = format!(
        "target_repo: {}\nhandoff_path: {}\nworkflow_template: {}\ndepends_on: [{}]{}",
        request.target_repo,
        request.handoff_path,
        request.workflow_template,
        request.depends_on.join(", "),
        request
            .agent_tier_override
            .map(|t| format!("\nagent_tier_override: {t:?}"))
            .unwrap_or_default()
    );

    let required_role = request.agent_tier_override.map(AgentRole::from);
    let options = TaskCreationOptions {
        required_role,
        workflow_id: Some(request.workflow_template.clone()),
        ..TaskCreationOptions::default()
    };

    let task = store
        .create_task_with_options(
            &title,
            Some(&description),
            requested_by,
            &request.project_root,
            &options,
        )
        .context("creating dispatch task")?;

    store
        .update_task_triage(
            &task.task_id,
            requested_by,
            TaskTriageUpdate {
                priority: Some(TaskPriority::from(request.priority)),
                ..TaskTriageUpdate::default()
            },
        )
        .context("applying dispatch priority")?;

    Ok(DispatchResponse {
        task_id: task.task_id,
        workflow_template: request.workflow_template.clone(),
        priority_applied: format!("{:?}", TaskPriority::from(request.priority)).to_lowercase(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn open_store() -> Store {
        Store::open(Path::new(":memory:")).expect("in-memory store")
    }

    #[test]
    fn intake_creates_task_with_correct_priority() {
        let store = open_store();
        let request = DispatchRequest {
            schema_version: "1.0".to_string(),
            handoff_path: ".handoffs/hymenium/handoff-parser.md".to_string(),
            workflow_template: "impl-audit".to_string(),
            project_root: "/tmp/test".to_string(),
            target_repo: "hymenium".to_string(),
            priority: DispatchPriority::High,
            depends_on: vec![],
            agent_tier_override: None,
        };

        let resp = intake(&store, &request, "hymenium").expect("intake should succeed");

        assert!(!resp.task_id.is_empty());
        assert_eq!(resp.workflow_template, "impl-audit");
        assert_eq!(resp.priority_applied, "high");

        let task = store.get_task(&resp.task_id).expect("task must exist");
        assert_eq!(task.priority, TaskPriority::High);
        assert_eq!(task.workflow_id.as_deref(), Some("impl-audit"));
    }

    #[test]
    fn intake_rejects_wrong_schema_version() {
        let store = open_store();
        let request = DispatchRequest {
            schema_version: "2.0".to_string(),
            handoff_path: ".handoffs/test.md".to_string(),
            workflow_template: "impl-audit".to_string(),
            project_root: "/tmp".to_string(),
            target_repo: "test".to_string(),
            priority: DispatchPriority::Medium,
            depends_on: vec![],
            agent_tier_override: None,
        };

        let err = intake(&store, &request, "hymenium").unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported dispatch-request schema version"),
            "error should mention schema version, got: {err}"
        );
    }

    #[test]
    fn intake_maps_agent_tier_to_required_role() {
        let store = open_store();
        let request = DispatchRequest {
            schema_version: "1.0".to_string(),
            handoff_path: ".handoffs/test.md".to_string(),
            workflow_template: "impl-audit".to_string(),
            project_root: "/tmp/test".to_string(),
            target_repo: "test".to_string(),
            priority: DispatchPriority::Medium,
            depends_on: vec![],
            agent_tier_override: Some(DispatchAgentTier::Opus),
        };

        let resp = intake(&store, &request, "operator").expect("intake should succeed");
        let task = store.get_task(&resp.task_id).expect("task must exist");
        assert_eq!(task.required_role, Some(AgentRole::Orchestrator));
    }

    #[test]
    fn intake_accepts_septa_fixture() {
        let store = open_store();
        let fixture = r#"{
            "schema_version": "1.0",
            "handoff_path": ".handoffs/hymenium/handoff-parser.md",
            "workflow_template": "impl-audit",
            "project_root": "/Users/williamnewton/projects/basidiocarp",
            "target_repo": "hymenium",
            "priority": "high",
            "depends_on": []
        }"#;

        let request: DispatchRequest =
            serde_json::from_str(fixture).expect("septa fixture must parse");
        let resp = intake(&store, &request, "septa-test").expect("intake should succeed");
        assert!(!resp.task_id.is_empty());
        assert_eq!(resp.priority_applied, "high");
    }

    #[test]
    fn dispatch_priority_converts_to_task_priority() {
        assert_eq!(TaskPriority::from(DispatchPriority::Low), TaskPriority::Low);
        assert_eq!(
            TaskPriority::from(DispatchPriority::Medium),
            TaskPriority::Medium
        );
        assert_eq!(
            TaskPriority::from(DispatchPriority::High),
            TaskPriority::High
        );
        assert_eq!(
            TaskPriority::from(DispatchPriority::Critical),
            TaskPriority::Critical
        );
    }

    #[test]
    fn validate_path_field_accepts_plausible_path() {
        validate_path_field("handoff_path", ".handoffs/hymenium/handoff-parser.md")
            .expect("a non-empty newline-free path must be accepted");
        validate_path_field("project_root", "/Users/williamnewton/projects/basidiocarp")
            .expect("a non-empty newline-free path must be accepted");
    }

    #[test]
    fn validate_path_field_rejects_empty() {
        let err =
            validate_path_field("handoff_path", "").expect_err("empty path field must be rejected");
        assert!(err.to_string().contains("handoff_path"));
        assert!(err.to_string().contains("not file contents"));
    }

    #[test]
    fn validate_path_field_rejects_embedded_newline() {
        // A multi-line value is the signature of file contents substituted for a path.
        let err = validate_path_field("project_root", "# Handoff\n\nsome file body")
            .expect_err("path field with a newline must be rejected");
        assert!(err.to_string().contains("project_root"));
        assert!(err.to_string().contains("newline"));
    }

    // Unique temp path for an end-to-end `read_request` test, isolated per process
    // and call site so parallel tests do not collide.
    fn temp_request_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "canopy-dispatch-{}-{}.json",
            std::process::id(),
            tag
        ))
    }

    #[test]
    fn read_request_accepts_septa_fixture_from_file() {
        let fixture = r#"{
            "schema_version": "1.0",
            "handoff_path": ".handoffs/hymenium/handoff-parser.md",
            "workflow_template": "impl-audit",
            "project_root": "/Users/williamnewton/projects/basidiocarp",
            "target_repo": "hymenium",
            "priority": "high",
            "depends_on": []
        }"#;
        let path = temp_request_path("valid");
        std::fs::write(&path, fixture).expect("write fixture");
        let result = read_request(path.to_str().expect("utf-8 path"));
        let _ = std::fs::remove_file(&path);
        let request = result.expect("valid septa fixture path must be accepted by read_request");
        assert_eq!(request.handoff_path, ".handoffs/hymenium/handoff-parser.md");
        assert_eq!(
            request.project_root,
            "/Users/williamnewton/projects/basidiocarp"
        );
    }

    #[test]
    fn read_request_rejects_content_substituted_for_path() {
        // handoff_path carries multi-line file contents instead of a path: read_request
        // must reject it (proves validation is wired into the real code path, not just
        // the standalone helper).
        let payload = "{\n            \"schema_version\": \"1.0\",\n            \"handoff_path\": \"# Handoff\\n\\nfile body not a path\",\n            \"workflow_template\": \"impl-audit\",\n            \"project_root\": \"/tmp/project\",\n            \"target_repo\": \"hymenium\",\n            \"priority\": \"high\",\n            \"depends_on\": []\n        }";
        let path = temp_request_path("content-as-path");
        std::fs::write(&path, payload).expect("write payload");
        let result = read_request(path.to_str().expect("utf-8 path"));
        let _ = std::fs::remove_file(&path);
        let err = result.expect_err("content substituted for handoff_path must be rejected");
        assert!(err.to_string().contains("handoff_path"));
        assert!(err.to_string().contains("not file contents"));
    }
}
