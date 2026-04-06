// Tools exported from this module:
// - tool_import_handoff

use crate::models::{AgentRole, EvidenceSourceKind};
use crate::scope::extract_step_scope;
use crate::store::{CanopyStore, EvidenceLinkRefs, TaskCreationOptions};
use crate::tools::{ToolResult, get_str, validate_required_string};
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
    let file_path_str = match validate_required_string(args, "file_path") {
        Ok(v) => v,
        Err(e) => return e,
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

    if let Some(agent_id) = assign_to {
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
    }

    let result = ImportHandoffResult {
        subtasks_created: subtasks.len(),
        parent_task_id: parent_task.task_id,
        subtasks,
    };
    ToolResult::json(&result)
}
