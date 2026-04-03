use serde_json::{Value, json};

use crate::store::CanopyStore;

use super::{ToolResult, get_str, get_string_array, validate_required_string};

/// Declare intent to modify files. Returns conflicts if any exist.
pub fn tool_files_lock(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(id) => id,
        Err(err) => return err,
    };

    let files = get_string_array(args, "files");
    if files.is_empty() {
        return ToolResult::error("missing required parameter: files".to_string());
    }

    let worktree_id = get_str(args, "worktree_id").unwrap_or("main");
    let file_count = files.len();

    match store.lock_files(agent_id, task_id, &files, worktree_id) {
        Ok(conflicts) if !conflicts.is_empty() => ToolResult::json(&json!({
            "locked": false,
            "conflicts": conflicts,
        })),
        Ok(_) => ToolResult::json(&json!({
            "locked": true,
            "files_locked": file_count,
        })),
        Err(err) => ToolResult::error(format!("lock failed: {err}")),
    }
}

/// Release file locks for a task.
pub fn tool_files_unlock(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(id) => id,
        Err(err) => return err,
    };

    match store.unlock_files(task_id) {
        Ok(released) => ToolResult::json(&json!({ "released": released })),
        Err(err) => ToolResult::error(format!("unlock failed: {err}")),
    }
}

/// Check for conflicts without locking.
pub fn tool_files_check(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let files = get_string_array(args, "files");
    if files.is_empty() {
        return ToolResult::error("missing required parameter: files".to_string());
    }

    let worktree_id = get_str(args, "worktree_id").unwrap_or("main");

    match store.check_file_conflicts(&files, worktree_id, Some(agent_id)) {
        Ok(conflicts) => ToolResult::json(&json!({
            "conflicts": conflicts,
            "all_clear": conflicts.is_empty(),
        })),
        Err(err) => ToolResult::error(format!("check failed: {err}")),
    }
}

/// List all active file locks.
pub fn tool_files_list_locks(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let project_root = get_str(args, "project_root");
    let filter_agent_id = get_str(args, "agent_id");

    match store.list_file_locks(project_root, filter_agent_id) {
        Ok(locks) => ToolResult::json(&locks),
        Err(err) => ToolResult::error(format!("list locks failed: {err}")),
    }
}
