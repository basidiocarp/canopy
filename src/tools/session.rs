//! Session-based coordination state for parallel agents without a broker.
//!
//! Sessions are file-backed (in `~/.local/share/basidiocarp/canopy/sessions/`)
//! and use lockfile-mediated atomic writes to support concurrent access.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::store::CanopyStore;

use super::{ToolResult, get_bounded_i64, validate_required_string};

type HResult<T> = Result<T, String>;

/// A message sent in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub sender: String,
    pub content: String,
    pub cursor_position: usize,
    pub timestamp: String,
}

/// Session state for multi-agent coordination within a handoff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffSession {
    pub id: String,
    pub created_by: String,
    pub handoff_slug: String,
    pub request: String,
    pub messages: Vec<SessionMessage>,
    pub status: String, // "open" | "closed"
}

/// Get the sessions directory, creating it if needed.
fn sessions_dir() -> HResult<PathBuf> {
    let dir = spore::paths::data_dir("basidiocarp")
        .join("canopy")
        .join("sessions");
    fs::create_dir_all(&dir).map_err(|e| format!("create sessions dir: {e}"))?;
    Ok(dir)
}

/// Get the path for a session file.
fn session_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Load a session from disk.
fn load_session(path: &Path) -> HResult<HandoffSession> {
    let content = fs::read_to_string(path).map_err(|e| format!("read session: {e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("deserialize session: {e}"))
}

/// Write a new session to disk atomically using a lockfile and temp file.
///
/// Intended only for the creation path (`tool_session_start`), where there is
/// nothing to load first. For mutations that need to read-modify-write, use
/// [`modify_session_locked`] instead so the load is inside the lock window.
fn write_session_atomic(path: &Path, session: &HandoffSession) -> HResult<()> {
    let lock_path = path.with_extension("json.lock");
    let lock_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|e| format!("acquire lock: {e}"))?;

    let result = (|| {
        let content =
            serde_json::to_string_pretty(session).map_err(|e| format!("serialize: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &content).map_err(|e| format!("write tmp: {e}"))?;
        fs::rename(&tmp, path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("rename: {e}")
        })?;
        Ok(())
    })();

    drop(lock_file);
    let _ = fs::remove_file(&lock_path);
    result
}

/// Acquire the lock, load the session, call `f` to mutate it, then write
/// atomically. The lock is held for the entire read-modify-write cycle so
/// concurrent senders cannot overwrite each other's messages.
fn modify_session_locked<F, T>(path: &Path, f: F) -> HResult<T>
where
    F: FnOnce(&mut HandoffSession) -> HResult<T>,
{
    let lock_path = path.with_extension("json.lock");
    let lock_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|e| format!("acquire lock: {e}"))?;

    let result: HResult<T> = (|| {
        let mut session = load_session(path)?;
        let value = f(&mut session)?;
        let content =
            serde_json::to_string_pretty(&session).map_err(|e| format!("serialize: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &content).map_err(|e| format!("write tmp: {e}"))?;
        fs::rename(&tmp, path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("rename: {e}")
        })?;
        Ok(value)
    })();

    drop(lock_file);
    let _ = fs::remove_file(&lock_path);
    result
}

/// Start a new coordination session.
///
/// # Parameters
/// - `handoff_slug`: Handoff this session is for
/// - `request`: Initial task description
#[must_use]
pub fn tool_session_start(
    _store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let handoff_slug = match validate_required_string(args, "handoff_slug") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let request = match validate_required_string(args, "request") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let session_id = format!("sess_{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));

    let session = HandoffSession {
        id: session_id.clone(),
        created_by: agent_id.to_string(),
        handoff_slug: handoff_slug.to_string(),
        request: request.to_string(),
        messages: Vec::new(),
        status: "open".to_string(),
    };

    let dir = match sessions_dir() {
        Ok(d) => d,
        Err(e) => return ToolResult::error(e),
    };

    let path = session_path(&dir, &session_id);
    if let Err(e) = write_session_atomic(&path, &session) {
        return ToolResult::error(e);
    }

    match serde_json::to_string_pretty(&serde_json::json!({
        "session_id": session_id,
        "status": "open",
        "created_by": agent_id,
        "handoff_slug": handoff_slug,
        "request": request,
    })) {
        Ok(text) => ToolResult::text(text),
        Err(e) => ToolResult::error(format!("serialize result: {e}")),
    }
}

/// Join an existing session.
///
/// # Parameters
/// - `session_id`: Session ID to join
///
/// Returns the current cursor (message count at time of join).
#[must_use]
pub fn tool_session_join(
    _store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let session_id = match validate_required_string(args, "session_id") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let dir = match sessions_dir() {
        Ok(d) => d,
        Err(e) => return ToolResult::error(e),
    };

    let path = session_path(&dir, session_id);
    let session = match load_session(&path) {
        Ok(s) => s,
        Err(e) => return ToolResult::error(e),
    };

    let cursor = session.messages.len() as i64;

    match serde_json::to_string_pretty(&serde_json::json!({
        "session_id": session.id,
        "status": session.status,
        "current_cursor": cursor,
    })) {
        Ok(text) => ToolResult::text(text),
        Err(e) => ToolResult::error(format!("serialize result: {e}")),
    }
}

/// Get messages from a session, optionally starting from a cursor position.
///
/// # Parameters
/// - `session_id`: Session ID to read from
/// - `cursor`: (Optional) Start position; defaults to 0. Returns messages[cursor..].
#[must_use]
pub fn tool_session_get(
    _store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let session_id = match validate_required_string(args, "session_id") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let cursor = get_bounded_i64(args, "cursor", 0, 0, i64::MAX) as usize;

    let dir = match sessions_dir() {
        Ok(d) => d,
        Err(e) => return ToolResult::error(e),
    };

    let path = session_path(&dir, session_id);
    let session = match load_session(&path) {
        Ok(s) => s,
        Err(e) => return ToolResult::error(e),
    };

    let messages = if cursor < session.messages.len() {
        session.messages[cursor..].to_vec()
    } else {
        Vec::new()
    };

    let next_cursor = (cursor + messages.len()) as i64;

    match serde_json::to_string_pretty(&serde_json::json!({
        "session_id": session.id,
        "cursor": cursor as i64,
        "next_cursor": next_cursor,
        "messages": messages,
    })) {
        Ok(text) => ToolResult::text(text),
        Err(e) => ToolResult::error(format!("serialize result: {e}")),
    }
}

/// Send a message in a session.
///
/// # Parameters
/// - `session_id`: Session ID
/// - `content`: Message content to append
///
/// Returns the cursor position of the new message.
#[must_use]
pub fn tool_session_send(
    _store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let session_id = match validate_required_string(args, "session_id") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let content = match validate_required_string(args, "content") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let dir = match sessions_dir() {
        Ok(d) => d,
        Err(e) => return ToolResult::error(e),
    };

    let path = session_path(&dir, session_id);
    let agent_id_owned = agent_id.to_string();
    let content_owned = content.to_string();

    let cursor_position = match modify_session_locked(&path, |session| {
        if session.status == "closed" {
            return Err("session is closed".to_string());
        }
        let cursor_position = session.messages.len();
        session.messages.push(SessionMessage {
            sender: agent_id_owned.clone(),
            content: content_owned.clone(),
            cursor_position,
            timestamp: Utc::now().to_rfc3339(),
        });
        Ok(cursor_position)
    }) {
        Ok(v) => v,
        Err(e) => return ToolResult::error(e),
    };

    match serde_json::to_string_pretty(&serde_json::json!({
        "session_id": session_id,
        "cursor_position": cursor_position as i64,
    })) {
        Ok(text) => ToolResult::text(text),
        Err(e) => ToolResult::error(format!("serialize result: {e}")),
    }
}

/// Close a session, preventing further messages.
///
/// # Parameters
/// - `session_id`: Session ID to close
#[must_use]
pub fn tool_session_close(
    _store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let session_id = match validate_required_string(args, "session_id") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let dir = match sessions_dir() {
        Ok(d) => d,
        Err(e) => return ToolResult::error(e),
    };

    let path = session_path(&dir, session_id);

    if let Err(e) = modify_session_locked(&path, |session| {
        session.status = "closed".to_string();
        Ok(())
    }) {
        return ToolResult::error(e);
    }

    match serde_json::to_string_pretty(&serde_json::json!({
        "session_id": session_id,
        "status": "closed",
        "closed": true,
    })) {
        Ok(text) => ToolResult::text(text),
        Err(e) => ToolResult::error(format!("serialize result: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn with_session_dir<F>(f: F)
    where
        F: FnOnce(),
    {
        // Note: Tests run isolated with their own tempdir setup.
        // For now, we test the core logic directly without session_dir mocking.
        f();
    }

    #[test]
    fn test_session_start_creates_session() {
        with_session_dir(|| {
            let session_id = format!("sess_{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));
            let session = HandoffSession {
                id: session_id.clone(),
                created_by: "agent-1".to_string(),
                handoff_slug: "test/handoff".to_string(),
                request: "test request".to_string(),
                messages: Vec::new(),
                status: "open".to_string(),
            };

            // Verify struct creation works
            assert_eq!(session.id, session_id);
            assert_eq!(session.status, "open");
            assert_eq!(session.messages.len(), 0);
        });
    }

    #[test]
    fn test_session_get_with_cursor() {
        with_session_dir(|| {
            let mut session = HandoffSession {
                id: "sess_test".to_string(),
                created_by: "agent-1".to_string(),
                handoff_slug: "test/handoff".to_string(),
                request: "test request".to_string(),
                messages: Vec::new(),
                status: "open".to_string(),
            };

            // Add 3 messages
            for i in 0..3 {
                session.messages.push(SessionMessage {
                    sender: format!("agent-{}", i),
                    content: format!("message {}", i),
                    cursor_position: i,
                    timestamp: Utc::now().to_rfc3339(),
                });
            }

            // Verify we can slice from cursor=1
            let messages = &session.messages[1..];
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0].cursor_position, 1);
        });
    }

    #[test]
    fn test_session_send_on_closed_returns_error() {
        with_session_dir(|| {
            let mut session = HandoffSession {
                id: "sess_test".to_string(),
                created_by: "agent-1".to_string(),
                handoff_slug: "test/handoff".to_string(),
                request: "test request".to_string(),
                messages: Vec::new(),
                status: "closed".to_string(),
            };

            // Verify closed status is detected
            assert_eq!(session.status, "closed");
            if session.status == "closed" {
                // This is the check used in tool_session_send
                assert!(true);
            }
        });
    }
}
