use serde::Serialize;

use crate::models::{Task, TaskStatus};
use crate::scope::{ScopeGap, classify_scope_gap, extract_step_scope, scope_overlaps};
use crate::store::{CanopyStore, StoreResult, TaskCreationOptions, TaskStatusUpdate};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScopeGapOutcome {
    InScope {
        task: Task,
        work_item: String,
    },
    NonBlocking {
        task: Task,
        work_item: String,
        scope_gap: ScopeGap,
    },
    Blocking {
        parent_task: Task,
        child_task: Task,
        work_item: String,
        scope_gap: ScopeGap,
    },
}

/// Apply the scope-detection protocol to a work item.
pub fn handle_scope_gap(
    store: &(impl CanopyStore + ?Sized),
    task_id: &str,
    agent_id: &str,
    work_item: &str,
) -> StoreResult<ScopeGapOutcome> {
    let task = store.get_task(task_id)?;
    let Some(scope_gap) = classify_scope_gap(work_item, &task.scope) else {
        let note = scope_gap_note("in_scope", None, work_item);
        let updated = store.update_task_status(
            task_id,
            task.status,
            agent_id,
            TaskStatusUpdate {
                blocked_reason: task.blocked_reason.as_deref(),
                event_note: Some(note.as_str()),
                ..TaskStatusUpdate::default()
            },
        )?;
        return Ok(ScopeGapOutcome::InScope {
            task: updated,
            work_item: work_item.to_string(),
        });
    };

    match scope_gap {
        ScopeGap::NonBlocking { description } => {
            let note = scope_gap_note("non_blocking", Some(&description), work_item);
            let updated = store.update_task_status(
                task_id,
                task.status,
                agent_id,
                TaskStatusUpdate {
                    blocked_reason: task.blocked_reason.as_deref(),
                    event_note: Some(note.as_str()),
                    ..TaskStatusUpdate::default()
                },
            )?;
            Ok(ScopeGapOutcome::NonBlocking {
                task: updated,
                work_item: work_item.to_string(),
                scope_gap: ScopeGap::NonBlocking { description },
            })
        }
        ScopeGap::Blocking { description } => {
            let child_scope = scope_gap_paths(work_item, &task.scope);
            let child_title = scope_gap_child_title(&description);
            let child_description = format!("{description}\n\n{work_item}");
            let child = store.create_subtask_with_options(
                task_id,
                &child_title,
                Some(&child_description),
                agent_id,
                &TaskCreationOptions {
                    required_role: task.required_role,
                    required_capabilities: task.required_capabilities.clone(),
                    auto_review: false,
                    verification_required: task.verification_required,
                    scope: child_scope,
                },
            )?;

            let blocked_reason = format!("{description}; child_task_id={}", child.task_id);
            let note = scope_gap_note("blocking", Some(&description), work_item);
            let parent = store.update_task_status(
                task_id,
                TaskStatus::Blocked,
                agent_id,
                TaskStatusUpdate {
                    blocked_reason: Some(blocked_reason.as_str()),
                    event_note: Some(note.as_str()),
                    ..TaskStatusUpdate::default()
                },
            )?;

            Ok(ScopeGapOutcome::Blocking {
                parent_task: parent,
                child_task: child,
                work_item: work_item.to_string(),
                scope_gap: ScopeGap::Blocking { description },
            })
        }
    }
}

#[must_use]
pub fn scope_gap_paths(work_item: &str, handoff_scope: &[String]) -> Vec<String> {
    extract_step_scope(work_item)
        .into_iter()
        .filter(|path| scope_overlaps(&[path.clone()], handoff_scope).is_empty())
        .collect()
}

fn scope_gap_note(kind: &str, description: Option<&str>, work_item: &str) -> String {
    let mut note = format!("scope_gap={kind}");
    if let Some(description) = description {
        note.push_str(&format!("; description={description}"));
    }
    let work_item = work_item.lines().next().unwrap_or(work_item).trim();
    if !work_item.is_empty() {
        note.push_str(&format!("; work_item={work_item}"));
    }
    note
}

fn scope_gap_child_title(description: &str) -> String {
    let summary = description
        .lines()
        .next()
        .unwrap_or(description)
        .trim()
        .trim_end_matches('.');
    let mut title = format!("Scope gap follow-up: {summary}");
    if title.len() > 120 {
        title.truncate(117);
        title.push_str("...");
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentRole;
    use crate::store::Store;
    use tempfile::tempdir;

    #[test]
    fn blocking_scope_gap_creates_child_and_blocks_parent() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("canopy.db");
        let store = Store::open(&db_path).expect("open store");
        let parent = store
            .create_task(
                "Parent task",
                Some("scope protocol"),
                "operator",
                "/tmp/project",
                Some(AgentRole::Implementer),
            )
            .expect("create task");

        let outcome = handle_scope_gap(
            &store,
            &parent.task_id,
            "operator",
            "Need to update `canopy/src/runtime.rs` before continuing",
        )
        .expect("handle scope gap");

        match outcome {
            ScopeGapOutcome::Blocking {
                parent_task,
                child_task,
                ..
            } => {
                assert_eq!(parent_task.status, TaskStatus::Blocked);
                assert!(
                    parent_task
                        .blocked_reason
                        .as_deref()
                        .is_some_and(|reason| reason.contains("child_task_id="))
                );
                assert_eq!(
                    child_task.parent_task_id.as_deref(),
                    Some(parent.task_id.as_str())
                );
            }
            other => panic!("expected blocking outcome, got {other:?}"),
        }
    }

    #[test]
    fn non_blocking_scope_gap_keeps_task_active() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("canopy.db");
        let store = Store::open(&db_path).expect("open store");
        let task = store
            .create_task("Parent task", None, "operator", "/tmp/project", None)
            .expect("create task");

        let outcome = handle_scope_gap(
            &store,
            &task.task_id,
            "operator",
            "Optional follow-up: draft `canopy/docs/scope-gap.md` later",
        )
        .expect("handle scope gap");

        match outcome {
            ScopeGapOutcome::NonBlocking { task, .. } => {
                assert_eq!(task.status, TaskStatus::Open);
                assert!(task.blocked_reason.is_none());
                assert!(
                    store
                        .get_children(&task.task_id)
                        .expect("children")
                        .is_empty()
                );
            }
            other => panic!("expected non-blocking outcome, got {other:?}"),
        }
    }
}
