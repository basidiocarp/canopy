// Tools exported from this module:
// - tool_import_handoff

use crate::models::{AgentRole, EvidenceSourceKind};
use crate::runtime::{DispatchDecision, pre_dispatch_check};
use crate::scope::extract_step_scope;
use crate::store::{CanopyStore, EvidenceLinkRefs, TaskCreationOptions};
use crate::tools::{ToolResult, get_str};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tracing::warn;

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
    warnings: Vec<String>,
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

/// Check whether a path has a `.handoffs` directory as one of its ancestors.
/// Returns `true` when the path is under a `.handoffs` tree, `false` otherwise.
fn path_is_under_handoffs_dir(path: &Path) -> bool {
    path.ancestors()
        .any(|a| a.file_name().and_then(|n| n.to_str()) == Some(".handoffs"))
}

/// Resolve a path to its canonical, normalized form for security checks.
///
/// Tries `std::fs::canonicalize` first (resolves symlinks, requires the path
/// to exist). If that fails (e.g. the file does not yet exist), falls back to
/// a purely lexical normalization that strips `.` and `..` components. This
/// ensures the ancestor check in `path_is_under_handoffs_dir` runs on the
/// real resolved path, not a raw string that might contain `..` escapes.
fn resolve_for_security_check(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    // Lexical fallback: walk components and collapse `.` / `..`.
    let mut components: Vec<std::ffi::OsString> = Vec::new();
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Don't pop past an absolute root or an empty stack.
                if components
                    .last()
                    .is_some_and(|c| c != std::ffi::OsStr::new("/"))
                {
                    components.pop();
                }
            }
            other => components.push(other.as_os_str().to_owned()),
        }
    }
    components.iter().collect()
}

/// Validate the handoff path and collect advisory warnings.
///
/// Returns `Err` when the path is outside any `.handoffs` directory — that is a
/// hard rejection because allowing arbitrary paths lets MCP callers read files
/// anywhere on the server filesystem. The remaining checks produce warnings only.
///
/// The path is resolved (symlinks and `..` components collapsed) before the
/// ancestor check to prevent traversal bypasses like `.handoffs/../../etc/passwd`.
fn validate_handoff_path(path: &Path) -> Result<Vec<String>, String> {
    // Resolve symlinks and collapse `..` before the ancestor check so that
    // a path like `.handoffs/../../etc/passwd` cannot sneak past the guard by
    // containing `.handoffs` as a raw component.
    let resolved = resolve_for_security_check(path);

    // Hard rejection: the resolved path must be inside a .handoffs directory tree.
    if !path_is_under_handoffs_dir(&resolved) {
        return Err(format!(
            "rejected: path '{}' is outside any .handoffs directory. \
             Handoff files must live under a .handoffs/ tree.",
            path.display()
        ));
    }

    let mut warnings = Vec::new();

    // Advisory: file should be in a project subdirectory, not .handoffs/ root
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

    // Advisory: verify script should exist alongside
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let verify_script = path.with_file_name(format!("verify-{stem}.sh"));
    if !verify_script.exists() {
        warnings.push(format!(
            "No verify script found. Expected: {}",
            verify_script.display()
        ));
    }

    // Advisory: old HANDOFF- prefix
    if stem.starts_with("HANDOFF-") {
        warnings.push(format!(
            "Uses old HANDOFF- prefix. Rename to: {}",
            stem.strip_prefix("HANDOFF-").unwrap_or(stem).to_lowercase()
        ));
    }

    Ok(warnings)
}

/// Import a .handoffs/ markdown file as a task with subtasks.
#[allow(clippy::too_many_lines)]
pub fn tool_import_handoff(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let Some(file_path_str) = get_str(args, "path").or_else(|| get_str(args, "file_path")) else {
        return ToolResult::error("missing required parameter: path".to_string());
    };
    let assign_to = get_str(args, "assign_to");

    let path = Path::new(file_path_str);

    // Hard rejection when the path is outside a .handoffs directory tree.
    // Advisory warnings (wrong subdirectory, missing verify script, old prefix)
    // are collected from the Ok variant and logged below.
    let warnings = match validate_handoff_path(path) {
        Ok(w) => w,
        Err(reason) => return ToolResult::error(reason),
    };
    for w in &warnings {
        warn!(path = %path.display(), "handoff import warning: {w}");
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
            Err(e) => {
                // Roll back the already-created parent task so callers never
                // observe a partial task tree. Foreign-key cascade removes all
                // related records (evidence, relationships, file_locks, etc.).
                if let Err(rollback_err) = store.delete_task(&parent_task.task_id) {
                    warn!(
                        task_id = %parent_task.task_id,
                        "rollback failed after subtask creation error: {rollback_err}"
                    );
                }
                return ToolResult::error(format!(
                    "failed to create subtask; rolled back parent task {}: {e}",
                    parent_task.task_id
                ));
            }
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
                if let Err(e) =
                    crate::store::ensure_capabilities_match(store, &parent_task.task_id, agent_id)
                {
                    return ToolResult::error(format!(
                        "import succeeded (task {}) but cannot assign to '{}': {e}. Task created but unassigned.",
                        parent_task.task_id, agent_id
                    ));
                }
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
                warn!(path = %path.display(), "holding handoff for human review: {reason}");
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
        warnings,
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
        #[cfg(windows)]
        let cortina_path = bin_dir.join("cortina.cmd");
        #[cfg(not(windows))]
        let cortina_path = bin_dir.join("cortina");

        #[cfg(windows)]
        let cortina_stub =
            "@echo off\r\necho {\"status\":\"flag_review\",\"reason\":\"stale handoff\"}\r\n";
        #[cfg(not(windows))]
        let cortina_stub = "#!/bin/sh\nprintf '%s\\n' '{\"status\":\"flag_review\",\"reason\":\"stale handoff\"}'\n";
        fs::write(&cortina_path, cortina_stub).expect("write cortina stub");

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
        let mut path_entries = vec![bin_dir.clone().into_os_string()];
        if let Some(existing) = &old_path {
            path_entries
                .extend(std::env::split_paths(existing).map(std::path::PathBuf::into_os_string));
        }
        let joined_path = std::env::join_paths(path_entries).expect("join PATH entries");

        unsafe {
            std::env::set_var("PATH", joined_path);
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

        assert!(
            !result.is_error,
            "unexpected error result: {:?}",
            result.content
        );

        let payload: serde_json::Value =
            serde_json::from_str(&result.content[0].text).expect("json result");
        let parent_task_id = payload["parent_task_id"].as_str().expect("parent task id");

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

        assert!(
            !result.is_error,
            "unexpected error result: {:?}",
            result.content
        );

        let payload: serde_json::Value =
            serde_json::from_str(&result.content[0].text).expect("json result");
        assert!(payload["parent_task_id"].as_str().is_some());
        assert_eq!(payload["requested_assignee"], serde_json::Value::Null);
    }

    #[test]
    fn import_handoff_rejects_path_outside_handoffs_tree() {
        let temp = tempdir().expect("tempdir");
        let rogue_path = temp.path().join("rogue.md");
        fs::write(
            &rogue_path,
            "# Handoff: Rogue\n\n### Step 1: Evil\nDo bad things.\n",
        )
        .expect("write rogue file");

        let db_path = temp.path().join("canopy.db");
        let store = Store::open(&db_path).expect("open store");

        let result = tool_import_handoff(
            &store,
            "agent-1",
            &json!({ "path": rogue_path.display().to_string() }),
        );

        assert!(
            result.is_error,
            "expected rejection for path outside .handoffs: {:?}",
            result.content
        );
        // Confirm no task was created.
        let tasks = store.list_tasks().expect("list tasks");
        assert!(
            tasks.is_empty(),
            "no tasks should exist after a rejected import"
        );
    }

    /// Verify that `delete_task` removes the parent and (via FK cascade) all
    /// related records, which is the mechanism the rollback relies on.
    #[test]
    fn import_rolls_back_partial_task_tree_on_subtask_failure() {
        // This test verifies the rollback path indirectly: it directly exercises
        // create_task_with_options + delete_task to confirm the store machinery
        // that tool_import_handoff relies on for rollback actually works.
        use crate::store::TaskCreationOptions;

        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("canopy.db");
        let store = Store::open(&db_path).expect("open store");

        // Create a parent task — simulates the first step of import.
        let parent = store
            .create_task_with_options(
                "Test Rollback Parent",
                Some("created for rollback test"),
                "test",
                "/tmp",
                &TaskCreationOptions::default(),
            )
            .expect("create parent task");

        // Confirm the task exists.
        assert!(
            store.get_task(&parent.task_id).is_ok(),
            "parent task should exist before rollback"
        );

        // Simulate a rollback by deleting the parent.
        store
            .delete_task(&parent.task_id)
            .expect("delete_task should succeed");

        // After rollback, the task must be gone.
        let tasks = store.list_tasks().expect("list tasks");
        assert!(
            tasks.is_empty(),
            "store should be empty after delete_task; got: {tasks:?}"
        );
    }

    /// A path that contains `.handoffs` as a *component* but escapes out of it
    /// via `..` must be rejected. Without path normalization the raw ancestor
    /// walk would find `.handoffs` and pass the check, then open an arbitrary
    /// file (e.g. `/etc/passwd`).
    #[test]
    fn import_rejects_path_traversal_through_handoffs_component() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();

        // Create the .handoffs directory and a target file outside it.
        let handoff_dir = root.join(".handoffs").join("canopy");
        fs::create_dir_all(&handoff_dir).expect("create handoff dir");
        let outside_file = root.join("secret.md");
        fs::write(&outside_file, "# Secret\n\nPrivate content.\n").expect("write outside file");

        // Construct a traversal path: starts under .handoffs, then escapes.
        // e.g. <root>/.handoffs/../../secret.md
        // After normalization this resolves to <root>/secret.md, which has no
        // .handoffs ancestor.
        let traversal_path = handoff_dir.join("..").join("..").join("secret.md");

        let db_path = root.join("canopy.db");
        let store = Store::open(&db_path).expect("open store");

        let result = tool_import_handoff(
            &store,
            "agent-1",
            &json!({ "path": traversal_path.display().to_string() }),
        );

        assert!(
            result.is_error,
            "expected rejection for traversal path '{}': {:?}",
            traversal_path.display(),
            result.content
        );
        // Confirm no task was created.
        let tasks = store.list_tasks().expect("list tasks");
        assert!(
            tasks.is_empty(),
            "no tasks should exist after a rejected import"
        );
    }
}
