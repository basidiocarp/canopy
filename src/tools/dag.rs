// Tools exported from this module:
// - tool_canopy_dag_create
// - tool_canopy_dag_add_node
// - tool_canopy_dag_add_edge
// - tool_canopy_dag_ready_nodes
// - tool_canopy_dag_complete_node

use crate::store::{CanopyStore, DagEdge, DagGraph, DagNode};
use crate::tools::{ToolResult, get_str, validate_required_string};
use serde::Serialize;
use serde_json::Value;
use ulid::Ulid;

#[derive(Debug, Serialize)]
struct CreateGraphResponse {
    graph_id: String,
}

#[derive(Debug, Serialize)]
struct AddNodeResponse {
    node_id: String,
}

#[derive(Debug, Serialize)]
struct AddEdgeResponse {
    edge_id: String,
}

#[derive(Debug, Serialize)]
struct NodeInfo {
    node_id: String,
    label: String,
    task_id: Option<String>,
    status: String,
}

#[derive(Debug, Serialize)]
struct CompleteNodeResponse {
    node_id: String,
    status: String,
}

/// Create a new DAG task graph.
///
/// # Arguments
///
/// * `name` - The name of the graph
///
/// # Returns
///
/// A newly generated `graph_id` string (ULID format).
#[allow(clippy::missing_errors_doc)]
pub fn tool_canopy_dag_create(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let name = match validate_required_string(args, "name") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let graph_id = Ulid::new().to_string();
    let now: i64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX);

    let graph = DagGraph {
        graph_id: graph_id.clone(),
        name: name.to_string(),
        status: "open".to_string(),
        created_at: now,
    };

    match store.dag_create_graph(&graph) {
        Ok(()) => ToolResult::json(&CreateGraphResponse { graph_id }),
        Err(e) => ToolResult::error(format!("failed to create graph: {e}")),
    }
}

/// Add a node to a DAG graph.
///
/// # Arguments
///
/// * `graph_id` - The ID of the graph
/// * `label` - The label for this node
/// * `task_id` - Optional task ID to associate with the node
///
/// # Returns
///
/// A newly generated `node_id` string (ULID format).
#[allow(clippy::missing_errors_doc)]
pub fn tool_canopy_dag_add_node(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let graph_id = match validate_required_string(args, "graph_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let label = match validate_required_string(args, "label") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let task_id = get_str(args, "task_id").map(String::from);

    let node_id = Ulid::new().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX);

    let node = DagNode {
        node_id: node_id.clone(),
        graph_id: graph_id.to_string(),
        label: label.to_string(),
        status: "pending".to_string(),
        task_id,
        created_at: now,
        completed_at: None,
    };

    match store.dag_add_node(&node) {
        Ok(()) => ToolResult::json(&AddNodeResponse { node_id }),
        Err(e) => ToolResult::error(format!("failed to add node: {e}")),
    }
}

/// Add a dependency edge between two nodes in a DAG graph.
///
/// # Arguments
///
/// * `graph_id` - The ID of the graph
/// * `from_node_id` - The source node ID
/// * `to_node_id` - The target node ID
/// * `edge_type` - Either `blocks` (default) or `informs` to describe the dependency
///
/// # Returns
///
/// A newly generated `edge_id` string (ULID format).
#[allow(clippy::missing_errors_doc)]
pub fn tool_canopy_dag_add_edge(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let graph_id = match validate_required_string(args, "graph_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let from_node_id = match validate_required_string(args, "from_node_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let to_node_id = match validate_required_string(args, "to_node_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let edge_type = get_str(args, "edge_type").unwrap_or("blocks");

    // Validate edge_type
    if edge_type != "blocks" && edge_type != "informs" {
        return ToolResult::error(format!(
            "invalid edge_type: {edge_type} (must be 'blocks' or 'informs')"
        ));
    }

    let edge_id = Ulid::new().to_string();

    let edge = DagEdge {
        edge_id: edge_id.clone(),
        graph_id: graph_id.to_string(),
        from_node_id: from_node_id.to_string(),
        to_node_id: to_node_id.to_string(),
        edge_type: edge_type.to_string(),
    };

    match store.dag_add_edge(&edge) {
        Ok(()) => ToolResult::json(&AddEdgeResponse { edge_id }),
        Err(e) => ToolResult::error(format!("failed to add edge: {e}")),
    }
}

/// Get all nodes in a graph that are ready to run (no blocking dependencies).
///
/// # Arguments
///
/// * `graph_id` - The ID of the graph
///
/// # Returns
///
/// An array of ready nodes, each containing `node_id`, `label`, `task_id`, and `status`.
#[allow(clippy::missing_errors_doc)]
pub fn tool_canopy_dag_ready_nodes(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let graph_id = match validate_required_string(args, "graph_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    match store.dag_get_ready_nodes(graph_id) {
        Ok(nodes) => {
            let info: Vec<NodeInfo> = nodes
                .into_iter()
                .map(|n| NodeInfo {
                    node_id: n.node_id,
                    label: n.label,
                    task_id: n.task_id,
                    status: n.status,
                })
                .collect();
            ToolResult::json(&info)
        }
        Err(e) => ToolResult::error(format!("failed to get ready nodes: {e}")),
    }
}

/// Mark a node as complete, freeing its dependents.
///
/// # Arguments
///
/// * `node_id` - The ID of the node to mark complete
///
/// # Returns
///
/// Confirmation with `node_id` and `status` set to `complete`.
#[allow(clippy::missing_errors_doc)]
pub fn tool_canopy_dag_complete_node(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let node_id = match validate_required_string(args, "node_id") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX);

    match store.dag_update_node_status(node_id, "complete", Some(now)) {
        Ok(()) => ToolResult::json(&CompleteNodeResponse {
            node_id: node_id.to_string(),
            status: "complete".to_string(),
        }),
        Err(e) => ToolResult::error(format!("failed to complete node: {e}")),
    }
}
