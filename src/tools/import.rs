// Tools exported from this module:
// - tool_import_handoff

use crate::models::{AgentRole, EvidenceSourceKind};
use crate::runtime::{DispatchDecision, pre_dispatch_check};
use crate::scope::extract_step_scope;
use crate::store::{CanopyStore, EvidenceLinkRefs, TaskCreationOptions};
use crate::tools::{ToolResult, get_str};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Serialize)]
struct ImportedSubtask {
    task_id: String,
    title: String,
}

#[derive(Debug, Serialize)]
struct ImportHandoffResult {
    parent_task_id: String,
    subtasks_created: usize,
    requested_assignee: Option<String>,
    assigned_to: Option<String>,
    review_hold_reason: Option<String>,
    subtasks: Vec<ImportedSubtask>,
}

struct ParsedStep {
    step_marker: String,
    description: Option<String>,
    scope: Vec<String>,
}

fn extract_title(content: &str) -> String {
    content
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("# Handoff:") {
                Some(trimmed.trim_start_matches("# Handoff:").trim().to_string())
            } else if trimmed.starts_with("# ") {
                Some(trimmed.trim_start_matches("# ").trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Untitled handoff".to_string())
}

fn extract_steps(content: &str) -> Vec<ParsedStep> {
    let mut steps = Vec::new();
    let mut current_marker: Option<String> = None;
    let mut current_body: Vec<String> = Vec::new();
    let mut skip_subsection = false;

    let flush = |steps: &mut Vec<ParsedStep>,
                 current_marker: &mut Option<String>,
                 current_body: &mut Vec<String>| {
        if let Some(step_marker) = current_marker.take() {
            let description = current_body.join("\n").trim().to_string();
            let scope = extract_step_scope(&description);
            steps.push(ParsedStep {
                description: (!description.is_empty()).then_some(description),
                step_marker,
                scope,
            });
            current_body.clear();
        }
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("### Step ") {
            flush(&mut steps, &mut current_marker, &mut current_body);
            current_marker = Some(trimmed.trim_start_matches("### ").trim().to_string());
            skip_subsection = false;
        } else if current_marker.is_some() && trimmed.starts_with("#### ") {
            skip_subsection = true;
        } else if current_marker.is_some() && !skip_subsection {
            current_body.push(line.to_string());
        }
    }
    flush(&mut steps, &mut current_marker, &mut current_body);
    steps
}

fn verify_script_path(path: &Path) -> std::path::PathBuf {
    let stem = path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("handoff");
    path.with_file_name(format!("verify-{stem}.sh"))
}

fn infer_project_root(path: &Path) -> String {
    for ancestor in path.ancestors() {
        if ancestor.file_name().and_then(|v| v.to_str()) == Some(".handoffs") {
            if let Some(project_root) = ancestor.parent() {
                return project_root.display().to_string();
            }
        }
    }
    std::env::current_dir().map_or_else(|_| ".".to_string(), |d| d.display().to_string())
}

fn validate_handoff_path(path: &Path) -> Vec<String> {
    let mut warnings = Vec::new();

    // Check: file should be in a project subdirectory, not .handoffs/ root
    let parent = path.parent().unwrap_or(Path::new("."));
    let parent_name = parent.file_name().and_then(|n| n.to_str()).unwrap_or("");

    if parent_name == ".handoffs" {
        warnings.push(format!(
            "Handoff is in .handoffs/ root. Move to .handoffs/<project>/{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
        ));
    }

    // Check: verify script should exist alongside
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let verify_script = path.with_file_name(format!("verify-{stem}.sh"));
    if !verify_script.exists() {
        warnings.push(format!(
            "No verify script found. Expected: {}",
            verify_script.display()
        ));
    }

    // Check: old HANDOFF- prefix
    if stem.starts_with("HANDOFF-") {
        warnings.push(format!(
            "Uses old HANDOFF- prefix. Rename to: {}",
            stem.strip_prefix("HANDOFF-").unwrap_or(stem).to_lowercase()
        ));
    }

    warnings
}

/// Import a .handoffs/ markdown file as a task with subtasks.
#[allow(clippy::too_many_lines)]
pub fn tool_import_handoff(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let file_path_str = match get_str(args, "path").or_else(|| get_str(args, "file_path")) {
        Some(v) => v,
        None => return ToolResult::error("missing required parameter: path".to_string()),
    };
    let assign_to = get_str(args, "assign_to");

    let path = Path::new(file_path_str);

    let warnings = validate_handoff_path(path);
    for w in &warnings {
        eprintln!("WARNING: {w}");
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return ToolResult::error(format!("failed to read file {file_path_str}: {e}")),
    };

    let title = extract_title(&content);
    let steps = extract_steps(&content);
    let verify_script = verify_script_path(path);
    let verify_script_exists = verify_script.exists();
    let project_root = infer_project_root(path);
    let parent_description = format!("Imported from {}", path.display());

    let parent_task = match store.create_task_with_options(
        &title,
        Some(&parent_description),
        "handoff-import",
        &project_root,
        &TaskCreationOptions {
            required_role: Some(AgentRole::Implementer),
            verification_required: true,
            ..TaskCreationOptions::default()
        },
    ) {
        Ok(t) => t,
        Err(e) => return ToolResult::error(format!("failed to create parent task: {e}")),
    };

    if verify_script_exists {
        let _ = store.add_evidence(
            &parent_task.task_id,
            EvidenceSourceKind::ManualNote,
            &path.display().to_string(),
            "Verification command",
            Some(&format!(
                "Run: canopy task verify --task-id {} --script {}",
                parent_task.task_id,
                verify_script.display()
            )),
            EvidenceLinkRefs::default(),
        );
    }

    let mut subtasks = Vec::new();
    for step in steps {
        let task = match store.create_subtask_with_options(
            &parent_task.task_id,
            &step.step_marker,
            step.description.as_deref(),
            "handoff-import",
            &TaskCreationOptions {
                required_role: Some(AgentRole::Implementer),
                verification_required: true,
                scope: step.scope,
                ..TaskCreationOptions::default()
            },
        ) {
            Ok(t) => t,
            Err(e) => return ToolResult::error(format!("failed to create subtask: {e}")),
        };

        if verify_script_exists {
            let _ = store.add_evidence(
                &task.task_id,
                EvidenceSourceKind::ManualNote,
                &path.display().to_string(),
                "Verification command",
                Some(&format!(
                    "Run: canopy task verify --task-id {} --script {} --step '{}'",
                    task.task_id,
                    verify_script.display(),
                    step.step_marker
                )),
                EvidenceLinkRefs::default(),
            );
        }

        subtasks.push(ImportedSubtask {
            task_id: task.task_id,
            title: task.title,
        });
    }

    let mut assigned_to = None;
    let mut review_hold_reason = None;
    if let Some(agent_id) = assign_to {
        match pre_dispatch_check(path) {
            DispatchDecision::Proceed => {
                if let Err(e) = store.assign_task(
                    &parent_task.task_id,
                    agent_id,
                    "handoff-import",
                    Some("assigned during handoff import"),
                ) {
                    return ToolResult::error(format!(
                        "import succeeded (task {}) but assignment to '{}' failed: {e}. Task created but unassigned.",
                        parent_task.task_id, agent_id
                    ));
                }
                assigned_to = Some(agent_id.to_string());
            }
            DispatchDecision::FlagForReview { reason } => {
                eprintln!("WARNING: holding handoff for human review: {reason}");
                review_hold_reason = Some(reason);
            }
        }
    }

    let result = ImportHandoffResult {
        subtasks_created: subtasks.len(),
        parent_task_id: parent_task.task_id,
        requested_assignee: assign_to.map(ToOwned::to_owned),
        assigned_to,
        review_hold_reason,
        subtasks,
    };
    ToolResult::json(&result)
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::tool_import_handoff;
    use crate::store::Store;
    use serde_json::json;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn import_handoff_reports_review_hold_without_assignment() {
        let _guard = env_lock().lock().expect("env lock");
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        let handoff_dir = root.join(".handoffs").join("cortina");
        fs::create_dir_all(&handoff_dir).expect("create handoff dir");

        let handoff_path = handoff_dir.join("demo.md");
        fs::write(
            &handoff_path,
            "# Handoff: Demo\n\n### Step 1: First\nImplement the step.\n",
        )
        .expect("write handoff");

        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).expect("create bin dir");
        let cortina_path = bin_dir.join("cortina");
        fs::write(
            &cortina_path,
            "#!/bin/sh\nprintf '%s\\n' '{\"status\":\"flag_review\",\"reason\":\"stale handoff\"}'\n",
        )
        .expect("write cortina stub");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&cortina_path)
                .expect("stub metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&cortina_path, permissions).expect("chmod cortina stub");
        }

        let old_path = std::env::var_os("PATH");
        let mut path_entries = vec![bin_dir.display().to_string()];
        if let Some(existing) = &old_path {
            path_entries.push(existing.to_string_lossy().into_owned());
        }

        unsafe {
            std::env::set_var("PATH", path_entries.join(":"));
        }

        let db_path = root.join("canopy.db");
        let store = Store::open(&db_path).expect("open store");

        let result = tool_import_handoff(
            &store,
            "agent-1",
            &json!({
                "file_path": handoff_path.display().to_string(),
                "assign_to": "agent-review"
            }),
        );

        match old_path {
            Some(value) => unsafe {
                std::env::set_var("PATH", value);
            },
            None => unsafe {
                std::env::remove_var("PATH");
            },
        }

        assert!(!result.is_error, "unexpected error result: {:?}", result.content);

        let payload: serde_json::Value =
            serde_json::from_str(&result.content[0].text).expect("json result");
        let parent_task_id = payload["parent_task_id"]
            .as_str()
            .expect("parent task id");

        assert_eq!(payload["requested_assignee"], "agent-review");
        assert!(payload["assigned_to"].is_null());
        assert_eq!(payload["review_hold_reason"], "stale handoff");

        let task = store.get_task(parent_task_id).expect("load parent task");
        assert!(
            task.owner_agent_id.is_none(),
            "held task should remain unassigned"
        );
    }

    #[test]
    fn import_handoff_accepts_schema_path_parameter() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        let handoff_dir = root.join(".handoffs").join("canopy");
        fs::create_dir_all(&handoff_dir).expect("create handoff dir");

        let handoff_path = handoff_dir.join("demo.md");
        fs::write(
            &handoff_path,
            "# Handoff: Demo\n\n### Step 1: First\nImplement the step.\n",
        )
        .expect("write handoff");

        let db_path = root.join("canopy.db");
        let store = Store::open(&db_path).expect("open store");

        let result = tool_import_handoff(
            &store,
            "agent-1",
            &json!({
                "path": handoff_path.display().to_string()
            }),
        );

        assert!(!result.is_error, "unexpected error result: {:?}", result.content);

        let payload: serde_json::Value =
            serde_json::from_str(&result.content[0].text).expect("json result");
        assert_eq!(payload["parent_task_id"].as_str().is_some(), true);
        assert_eq!(payload["requested_assignee"], serde_json::Value::Null);
    }
}
