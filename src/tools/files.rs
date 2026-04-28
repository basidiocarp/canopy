use serde_json::{Value, json};
use std::path::{Component, Path};

use crate::store::CanopyStore;

use super::{ToolResult, get_str, get_string_array, validate_required_string};

/// Normalize a file path by resolving `.`, `..`, and redundant separators.
///
/// Returns `None` if any path component cannot be represented as a valid
/// UTF-8 string. The result is an equivalent path with no `.` or `..`
/// components and no trailing slash (except for a lone `/`).
///
/// Two paths that refer to the same file (e.g. `"./foo/../bar"` and `"bar"`)
/// normalize to the same string, ensuring they map to the same lock slot.
fn normalize_path(raw: &str) -> Option<String> {
    let mut components: Vec<&str> = Vec::new();
    for component in Path::new(raw).components() {
        match component {
            Component::Prefix(p) => {
                let s = p.as_os_str().to_str()?;
                components.clear();
                components.push(s);
            }
            Component::RootDir => {
                components.clear();
                components.push("/");
            }
            Component::CurDir => {
                // `.` — no movement, skip
            }
            Component::ParentDir => {
                // `..` — pop one level unless already at root or empty
                if components.last().is_some_and(|&top| top != "/") {
                    components.pop();
                }
            }
            Component::Normal(name) => {
                let s = name.to_str()?;
                components.push(s);
            }
        }
    }

    if components.is_empty() {
        return Some(".".to_string());
    }

    // Reassemble with correct separators.
    let mut out = String::new();
    for &part in &components {
        if part == "/" {
            out.push('/');
        } else if out.is_empty() || out.ends_with('/') {
            out.push_str(part);
        } else {
            out.push('/');
            out.push_str(part);
        }
    }
    Some(out)
}

/// Declare intent to modify files. Returns conflicts if any exist.
///
/// All paths are normalized before being stored so that two different
/// spellings of the same file (e.g. `"./foo/../bar"` and `"bar"`) map to
/// the same lock slot and correctly detect each other as conflicts.
pub fn tool_files_lock(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(id) => id,
        Err(err) => return err,
    };

    let raw_files = get_string_array(args, "files");
    if raw_files.is_empty() {
        return ToolResult::error("missing required parameter: files".to_string());
    }

    // Normalize every path before storing. Reject non-normalizable paths so
    // two different spellings of the same file cannot bypass each other's locks.
    let mut canonical_files: Vec<String> = Vec::with_capacity(raw_files.len());
    for raw in &raw_files {
        match normalize_path(raw) {
            Some(canonical) => canonical_files.push(canonical),
            None => {
                return ToolResult::error(format!(
                    "invalid or non-canonical path rejected: '{raw}'"
                ));
            }
        }
    }

    let worktree_id = get_str(args, "worktree_id").unwrap_or("main");
    let file_count = canonical_files.len();

    match store.lock_files(agent_id, task_id, &canonical_files, worktree_id) {
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

/// Release file locks for a task, scoped to the calling agent.
///
/// By default only the locks owned by `agent_id` are released. An agent
/// cannot release another agent's locks without an explicit `force: true`
/// flag (operator override).
pub fn tool_files_unlock(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match validate_required_string(args, "task_id") {
        Ok(id) => id,
        Err(err) => return err,
    };

    let force = args
        .get("force")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    if force {
        // Operator override: release all locks for the task unconditionally.
        match store.unlock_files(task_id) {
            Ok(released) => ToolResult::json(&json!({ "released": released, "force": true })),
            Err(err) => ToolResult::error(format!("unlock failed: {err}")),
        }
    } else {
        // Normal path: only release locks owned by the calling agent.
        match store.unlock_files_for_agent(task_id, agent_id) {
            Ok(0) => ToolResult::error(format!(
                "no active locks found for task '{task_id}' owned by agent '{agent_id}'; \
                 pass force: true to release locks owned by other agents"
            )),
            Ok(released) => ToolResult::json(&json!({ "released": released, "force": false })),
            Err(err) => ToolResult::error(format!("unlock failed: {err}")),
        }
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

#[cfg(test)]
mod tests {
    use super::{normalize_path, tool_files_lock, tool_files_unlock};
    use crate::store::{Store, TaskCreationOptions};
    use serde_json::json;
    use tempfile::tempdir;

    /// Create a task and return its task_id string.
    fn create_task(store: &Store, title: &str) -> String {
        store
            .create_task_with_options(title, None, "test", "/tmp", &TaskCreationOptions::default())
            .expect("create task")
            .task_id
    }

    #[test]
    fn normalize_path_collapses_dot_dot_and_dot() {
        let a = normalize_path("./foo/../bar");
        let b = normalize_path("bar");
        assert_eq!(a, Some("bar".to_string()));
        assert_eq!(
            a, b,
            "dot-dot form and direct form must normalize to the same string"
        );
    }

    #[test]
    fn normalize_path_strips_trailing_slash_equivalent() {
        // Path::components() ignores trailing slashes; normalization should be stable.
        assert_eq!(normalize_path("a/b"), Some("a/b".to_string()));
        assert_eq!(normalize_path("./a/b"), Some("a/b".to_string()));
    }

    #[test]
    fn normalize_path_absolute() {
        assert_eq!(
            normalize_path("/foo/./bar/../baz"),
            Some("/foo/baz".to_string())
        );
    }

    #[test]
    fn lock_normalizes_paths_so_aliases_map_to_same_slot() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("canopy.db");
        let store = Store::open(&db_path).expect("open store");

        // Tasks must exist in the DB for the FK constraint.
        let task_a_id = create_task(&store, "Task A");
        let task_b_id = create_task(&store, "Task B");

        // Agent A locks "bar" via the canonical form.
        let result_a = tool_files_lock(
            &store,
            "agent-a",
            &json!({
                "task_id": task_a_id,
                "files": ["bar"],
                "worktree_id": "main"
            }),
        );
        assert!(
            !result_a.is_error,
            "agent-a lock should succeed: {:?}",
            result_a.content
        );
        let payload_a: serde_json::Value =
            serde_json::from_str(&result_a.content[0].text).expect("json");
        assert_eq!(payload_a["locked"], true);

        // Agent B tries to lock the same file using a dot-dot alias.
        let result_b = tool_files_lock(
            &store,
            "agent-b",
            &json!({
                "task_id": task_b_id,
                "files": ["./foo/../bar"],
                "worktree_id": "main"
            }),
        );
        assert!(
            !result_b.is_error,
            "lock call should not error: {:?}",
            result_b.content
        );
        let payload_b: serde_json::Value =
            serde_json::from_str(&result_b.content[0].text).expect("json");
        assert_eq!(
            payload_b["locked"], false,
            "agent-b must be blocked by agent-a's canonical lock"
        );
        assert!(
            payload_b["conflicts"]
                .as_array()
                .map_or(false, |c| !c.is_empty()),
            "conflicts must be reported"
        );
    }

    #[test]
    fn unlock_agent_b_cannot_release_agent_a_lock_without_force() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("canopy.db");
        let store = Store::open(&db_path).expect("open store");

        // Task must exist in the DB for the FK constraint.
        let task_id = create_task(&store, "Shared Task");

        // Agent A acquires a lock on the task.
        let lock_result = tool_files_lock(
            &store,
            "agent-a",
            &json!({
                "task_id": task_id,
                "files": ["src/main.rs"],
                "worktree_id": "main"
            }),
        );
        assert!(
            !lock_result.is_error,
            "agent-a lock failed: {:?}",
            lock_result.content
        );

        // Agent B tries to unlock without force — must be rejected (owns no locks).
        let unlock_result = tool_files_unlock(&store, "agent-b", &json!({ "task_id": task_id }));
        assert!(
            unlock_result.is_error,
            "agent-b must not be able to unlock agent-a's locks without force"
        );

        // Agent B uses force=true (operator override) — should succeed.
        let force_result = tool_files_unlock(
            &store,
            "agent-b",
            &json!({ "task_id": task_id, "force": true }),
        );
        assert!(
            !force_result.is_error,
            "force unlock should succeed: {:?}",
            force_result.content
        );
        let payload: serde_json::Value =
            serde_json::from_str(&force_result.content[0].text).expect("json");
        assert_eq!(payload["released"], 1, "one lock should have been released");
        assert_eq!(payload["force"], true);
    }
}
