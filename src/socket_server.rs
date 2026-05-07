//! Unix-socket endpoint for direct JSON-RPC 2.0 queries.
//!
//! Cap and other local clients use this endpoint to query canopy coordination data
//! without spawning a subprocess. Bind path is
//! `~/.local/share/basidiocarp/canopy/canopy.sock`. The endpoint
//! descriptor at `~/.config/canopy/canopy.endpoint.json` lets clients
//! discover the socket path via the `local-service-endpoint-v1` convention.
//!
//! # Supported methods
//!
//! - `PING` / `ping` — health probe, returns `{}`
//! - `canopy_snapshot` — full coordination snapshot with optional filters
//! - `canopy_task` — task detail for a specific task
//! - `canopy_agents` — list agents, optionally filtered by `project_root`

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Value, json};
use tracing::{debug, error};

const CAPABILITY_ID: &str = "coordination.read.v1";
const PING_METHOD: &str = "PING";

fn write_endpoint_descriptor(socket_path: &Path) -> Result<()> {
    let config_dir = spore::paths::config_dir("canopy");
    std::fs::create_dir_all(&config_dir)?;
    let descriptor_path = config_dir.join("canopy.endpoint.json");
    let descriptor = json!({
        "schema_version": "1.0",
        "transport": "unix-socket",
        "endpoint": socket_path.to_string_lossy(),
        "capability_id": CAPABILITY_ID,
        "version": env!("CARGO_PKG_VERSION"),
        "health_probe": { "method": PING_METHOD, "timeout_ms": 1000 }
    });
    std::fs::write(&descriptor_path, serde_json::to_string_pretty(&descriptor)?)?;
    Ok(())
}

fn remove_stale_socket(socket_path: &Path) {
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC helpers
// ---------------------------------------------------------------------------

fn ok_response(id: &Value, result: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err_response(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() }
    })
}

fn write_response(writer: &mut (impl Write + ?Sized), response: &Value) {
    if let Ok(bytes) = serde_json::to_vec(response) {
        let _ = writer.write_all(&bytes);
        let _ = writer.write_all(b"\n");
        let _ = writer.flush();
    }
}

// ---------------------------------------------------------------------------
// canopy_snapshot handler
// ---------------------------------------------------------------------------

fn handle_snapshot(params: &Value) -> Value {
    use canopy::api::{self, SnapshotOptions};
    use canopy::models::{
        AttentionLevel, SnapshotPreset, TaskPriority, TaskSeverity, TaskSort, TaskView,
    };

    #[derive(serde::Deserialize, Default)]
    struct SnapshotParams {
        project_root: Option<String>,
        preset: Option<SnapshotPreset>,
        sort: Option<TaskSort>,
        view: Option<TaskView>,
        priority_at_least: Option<TaskPriority>,
        severity_at_least: Option<TaskSeverity>,
        acknowledged: Option<bool>,
        attention_at_least: Option<AttentionLevel>,
    }

    let params: SnapshotParams = match serde_json::from_value(params.clone()) {
        Ok(p) => p,
        Err(e) => return json!({ "error": format!("param parse: {e}") }),
    };

    let store = match crate::db::open(None) {
        Ok(s) => s,
        Err(e) => return json!({ "error": format!("store open: {e}") }),
    };

    let options = SnapshotOptions {
        project_root: params.project_root.as_deref(),
        preset: params.preset,
        sort: params.sort,
        view: params.view,
        priority_at_least: params.priority_at_least,
        severity_at_least: params.severity_at_least,
        acknowledged: params.acknowledged,
        attention_at_least: params.attention_at_least,
    };

    match api::snapshot(&store, options) {
        Ok(snapshot) => serde_json::to_value(&snapshot).unwrap_or_else(|_| json!({})),
        Err(e) => json!({ "error": format!("snapshot: {e}") }),
    }
}

// ---------------------------------------------------------------------------
// canopy_task handler
// ---------------------------------------------------------------------------

fn handle_task(params: &Value) -> Value {
    use canopy::api;

    #[derive(serde::Deserialize)]
    struct TaskParams {
        task_id: String,
    }

    let params: TaskParams = match serde_json::from_value(params.clone()) {
        Ok(p) => p,
        Err(e) => return json!({ "error": format!("param parse: {e}") }),
    };

    let store = match crate::db::open(None) {
        Ok(s) => s,
        Err(e) => return json!({ "error": format!("store open: {e}") }),
    };

    match api::task_detail(&store, &params.task_id) {
        Ok(detail) => {
            let wire: canopy::models::TaskDetailWire = detail.into();
            serde_json::to_value(&wire).unwrap_or_else(|_| json!({}))
        }
        Err(e) => json!({ "error": format!("task detail: {e}") }),
    }
}

// ---------------------------------------------------------------------------
// canopy_agents handler
// ---------------------------------------------------------------------------

fn handle_agents(params: &Value) -> Value {
    #[derive(serde::Deserialize, Default)]
    struct AgentsParams {
        project_root: Option<String>,
    }

    let params: AgentsParams = match serde_json::from_value(params.clone()) {
        Ok(p) => p,
        Err(e) => return json!({ "error": format!("param parse: {e}") }),
    };

    let store = match crate::db::open(None) {
        Ok(s) => s,
        Err(e) => return json!({ "error": format!("store open: {e}") }),
    };

    let agents = if let Some(project_root) = params.project_root.as_deref() {
        match store.list_agents_filtered(Some(project_root)) {
            Ok(a) => a,
            Err(e) => return json!({ "error": format!("list agents: {e}") }),
        }
    } else {
        match store.list_agents() {
            Ok(a) => a,
            Err(e) => return json!({ "error": format!("list agents: {e}") }),
        }
    };

    match serde_json::to_value(&agents) {
        Ok(v) => v,
        Err(_) => json!({}),
    }
}

// ---------------------------------------------------------------------------
// Connection handler
// ---------------------------------------------------------------------------

fn handle_connection(stream: std::os::unix::net::UnixStream) {
    let writer_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            error!("failed to clone unix stream: {e}");
            return;
        }
    };
    let mut reader = BufReader::new(stream);
    let mut writer = writer_stream;

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) => {
                error!("socket read error: {e}");
                return;
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let resp = err_response(&Value::Null, -32700, format!("parse error: {e}"));
                write_response(&mut writer, &resp);
                return;
            }
        };

        let id = match msg.get("id").cloned() {
            Some(id) if !id.is_null() => id,
            _ => continue, // notification — no response
        };

        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        debug!("socket request: {method}");

        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

        let response = match method {
            m if m == PING_METHOD || m == "ping" => {
                let empty = json!({});
                ok_response(&id, &empty)
            }
            "canopy_snapshot" => {
                let result = handle_snapshot(&params);
                if result.get("error").is_some() {
                    let msg = result["error"]
                        .as_str()
                        .unwrap_or("snapshot error")
                        .to_string();
                    err_response(&id, -32000, msg)
                } else {
                    ok_response(&id, &result)
                }
            }
            "canopy_task" => {
                let result = handle_task(&params);
                if result.get("error").is_some() {
                    let msg = result["error"].as_str().unwrap_or("task error").to_string();
                    err_response(&id, -32000, msg)
                } else {
                    ok_response(&id, &result)
                }
            }
            "canopy_agents" => {
                let result = handle_agents(&params);
                if result.get("error").is_some() {
                    let msg = result["error"]
                        .as_str()
                        .unwrap_or("agents error")
                        .to_string();
                    err_response(&id, -32000, msg)
                } else {
                    ok_response(&id, &result)
                }
            }
            _ => err_response(&id, -32601, format!("method not found: {method}")),
        };

        write_response(&mut writer, &response);
    }
}

// ---------------------------------------------------------------------------
// Server entry point
// ---------------------------------------------------------------------------

/// Start the canopy unix-socket service endpoint.
///
/// Binds to `~/.local/share/basidiocarp/canopy/canopy.sock`, writes the
/// endpoint descriptor to `~/.config/canopy/canopy.endpoint.json`, then
/// accepts connections indefinitely. Each connection is handled in a
/// background thread.
pub fn run_socket_server() -> Result<()> {
    let socket_path: PathBuf = spore::paths::data_dir("basidiocarp")
        .join("canopy")
        .join("canopy.sock");

    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    remove_stale_socket(&socket_path);

    let listener = std::os::unix::net::UnixListener::bind(&socket_path).map_err(|e| {
        anyhow::anyhow!(
            "failed to bind canopy socket {}: {e}",
            socket_path.display()
        )
    })?;

    write_endpoint_descriptor(&socket_path)?;

    tracing::info!(
        socket = %socket_path.display(),
        "canopy socket endpoint ready"
    );

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                std::thread::spawn(move || handle_connection(stream));
            }
            Err(e) => error!("canopy socket accept error: {e}"),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use tempfile::TempDir;

    fn temp_socket_path(dir: &TempDir) -> PathBuf {
        dir.path().join("test.sock")
    }

    #[test]
    fn socket_server_ping_responds_ok() {
        let tmp = TempDir::new().unwrap();
        let socket_path = temp_socket_path(&tmp);

        remove_stale_socket(&socket_path);
        let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        let socket_path_clone = socket_path.clone();

        let handle = std::thread::spawn(move || {
            if let Ok(stream) = listener.accept().map(|(s, _)| s) {
                handle_connection(stream);
            }
        });

        let mut client = std::os::unix::net::UnixStream::connect(&socket_path_clone).unwrap();
        let request = r#"{"jsonrpc":"2.0","id":1,"method":"PING","params":null}"#;
        client.write_all(request.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        client.flush().unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();

        let reader = BufReader::new(&client);
        let line = reader.lines().next().expect("response").unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], 1);
        assert!(v.get("result").is_some());
        assert!(v.get("error").is_none());

        handle.join().unwrap();
    }

    #[test]
    fn socket_server_unknown_method_returns_method_not_found() {
        let tmp = TempDir::new().unwrap();
        let socket_path = temp_socket_path(&tmp);

        remove_stale_socket(&socket_path);
        let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        let socket_path_clone = socket_path.clone();

        let handle = std::thread::spawn(move || {
            if let Ok(stream) = listener.accept().map(|(s, _)| s) {
                handle_connection(stream);
            }
        });

        let mut client = std::os::unix::net::UnixStream::connect(&socket_path_clone).unwrap();
        let request = r#"{"jsonrpc":"2.0","id":2,"method":"no_such_method","params":{}}"#;
        client.write_all(request.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        client.flush().unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();

        let reader = BufReader::new(&client);
        let line = reader.lines().next().expect("response").unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], 2);
        assert!(v.get("error").is_some());
        assert_eq!(v["error"]["code"], -32601);

        handle.join().unwrap();
    }
}
