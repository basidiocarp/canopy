use std::io::{self, BufRead, Write};

use serde_json::{Value, json};
use spore::logging::{SpanContext, request_span, root_span, tool_span};
use tracing::{debug, error};

use crate::store::Store;

use super::protocol::{JsonRpcMessage, JsonRpcResponse};

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Run the MCP server on stdio. Blocks until stdin is closed.
///
/// # Errors
///
/// Returns an error if reading from stdin or writing to stdout fails.
pub fn run_server(
    store: &Store,
    agent_id: &str,
    project: Option<&str>,
    worktree: Option<&str>,
) -> anyhow::Result<()> {
    let _root_span = root_span(&server_span_context(project)).entered();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error!("stdin read error: {e}");
                break;
            }
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let msg: JsonRpcMessage = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(e) => {
                error!("invalid JSON-RPC: {e}");
                let resp = JsonRpcResponse::err(Value::Null, -32700, format!("parse error: {e}"));
                write_response(&mut stdout, &resp)?;
                continue;
            }
        };

        let method = msg.method.as_deref().unwrap_or_else(|| {
            error!("received JSON-RPC message without method field");
            ""
        });
        let request_context = request_span_context(project, msg.id.as_ref(), method);
        let _request_span = request_span(method, &request_context).entered();
        debug!("MCP request: {method}");

        // Notifications have no id — don't respond
        let Some(id) = msg.id else {
            debug!("notification received, ignoring: {method}");
            continue;
        };

        let response = match method {
            "initialize" => handle_initialize(id, agent_id, project, worktree),
            "ping" => JsonRpcResponse::ok(id, json!({})),
            "notifications/initialized" => {
                // Client acknowledgement — no response needed
                continue;
            }
            "tools/list" => handle_tools_list(id),
            "tools/call" => handle_tools_call(id, msg.params, store, agent_id, project),
            other => JsonRpcResponse::method_not_found(id, other),
        };

        write_response(&mut stdout, &response)?;
    }

    Ok(())
}

fn write_response(stdout: &mut io::Stdout, resp: &JsonRpcResponse) -> anyhow::Result<()> {
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, resp)?;
    lock.write_all(b"\n")?;
    lock.flush()?;
    Ok(())
}

fn handle_initialize(
    id: Value,
    agent_id: &str,
    project: Option<&str>,
    worktree: Option<&str>,
) -> JsonRpcResponse {
    let project_hint = project
        .map(|p| format!("\nProject root: {p}"))
        .unwrap_or_default();
    let worktree_hint = worktree
        .map(|w| format!("\nWorktree: {w}"))
        .unwrap_or_default();

    let instructions = format!(
        r#"You are agent "{agent_id}" in a Canopy multi-agent coordination system.{project_hint}{worktree_hint}

Use canopy tools to:
- Check for available work: canopy_work_queue
- Claim a task atomically: canopy_task_claim (fails if another agent got it first)
- Lock files before editing: canopy_files_lock
- Send heartbeats: canopy_heartbeat (every ~10 tool calls)
- Report scope gaps: canopy_report_scope_gap
- Read declared scope: canopy_get_handoff_scope
- Hand off work: canopy_handoff_create
- Complete tasks: canopy_task_complete

Workflow: canopy_work_queue → canopy_task_claim → canopy_files_lock → work → canopy_task_complete

All IDs are ULIDs. Timestamps are RFC3339 UTC."#
    );

    JsonRpcResponse::ok(
        id,
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "canopy",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": instructions
        }),
    )
}

fn handle_tools_list(id: Value) -> JsonRpcResponse {
    let tools = super::schema::tool_definitions();
    JsonRpcResponse::ok(id, json!({ "tools": tools }))
}

#[allow(clippy::needless_pass_by_value)]
fn handle_tools_call(
    id: Value,
    params: Option<Value>,
    store: &Store,
    agent_id: &str,
    project: Option<&str>,
) -> JsonRpcResponse {
    let Some(params) = params else {
        return JsonRpcResponse::err(id, -32602, "missing params".into());
    };

    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::err(id, -32602, "missing tool name".into());
    };

    debug!("tool call: {name}");
    let _tool_span = tool_span(name, &tool_span_context(project, &id, name)).entered();

    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let result = crate::tools::dispatch_tool(store, agent_id, name, &args);

    JsonRpcResponse::ok(id, json!(result))
}

fn server_span_context(project: Option<&str>) -> SpanContext {
    let context = SpanContext::for_app("canopy");
    match project {
        Some(project) if !project.trim().is_empty() => {
            context.with_workspace_root(project.to_string())
        }
        _ => context,
    }
}

fn request_span_context(project: Option<&str>, id: Option<&Value>, method: &str) -> SpanContext {
    let context = server_span_context(project).with_tool(method);
    match id.and_then(request_id_from_value) {
        Some(request_id) => context.with_request_id(request_id),
        None => context,
    }
}

fn tool_span_context(project: Option<&str>, id: &Value, tool_name: &str) -> SpanContext {
    let context = server_span_context(project).with_tool(tool_name);
    match request_id_from_value(id) {
        Some(request_id) => context.with_request_id(request_id),
        None => context,
    }
}

fn request_id_from_value(id: &Value) -> Option<String> {
    match id {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{request_id_from_value, request_span_context};
    use serde_json::json;

    #[test]
    fn request_id_from_value_supports_jsonrpc_scalars() {
        assert_eq!(
            request_id_from_value(&json!("req-1")).as_deref(),
            Some("req-1")
        );
        assert_eq!(request_id_from_value(&json!(42)).as_deref(), Some("42"));
        assert_eq!(request_id_from_value(&json!(true)).as_deref(), Some("true"));
        assert_eq!(request_id_from_value(&Value::Null), None);
    }

    #[test]
    fn request_span_context_carries_workspace_root_and_request_id() {
        let context = request_span_context(Some("/repo/demo"), Some(&json!("req-7")), "tools/call");
        assert_eq!(context.service.as_deref(), Some("canopy"));
        assert_eq!(context.tool.as_deref(), Some("tools/call"));
        assert_eq!(context.request_id.as_deref(), Some("req-7"));
        assert_eq!(context.workspace_root.as_deref(), Some("/repo/demo"));
    }

    use serde_json::Value;
}
