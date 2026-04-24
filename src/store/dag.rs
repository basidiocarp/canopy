use super::StoreResult;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagGraph {
    pub graph_id: String,
    pub name: String,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNode {
    pub node_id: String,
    pub graph_id: String,
    pub label: String,
    pub status: String,
    pub task_id: Option<String>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagEdge {
    pub edge_id: String,
    pub graph_id: String,
    pub from_node_id: String,
    pub to_node_id: String,
    pub edge_type: String,
}

/// Create a new DAG graph.
///
/// # Errors
///
/// Returns an error if the database write fails.
pub fn create_graph(conn: &Connection, g: &DagGraph) -> StoreResult<()> {
    conn.execute(
        "INSERT INTO dag_graphs (graph_id, name, status, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![g.graph_id, g.name, g.status, g.created_at],
    )?;
    Ok(())
}

/// Add a node to a DAG graph.
///
/// # Errors
///
/// Returns an error if the database write fails.
pub fn add_node(conn: &Connection, n: &DagNode) -> StoreResult<()> {
    conn.execute(
        "INSERT INTO dag_nodes (node_id, graph_id, label, status, task_id, created_at, completed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            n.node_id,
            n.graph_id,
            n.label,
            n.status,
            n.task_id,
            n.created_at,
            n.completed_at,
        ],
    )?;
    Ok(())
}

/// Add an edge (dependency) between two nodes.
///
/// # Errors
///
/// Returns an error if the database write fails.
pub fn add_edge(conn: &Connection, e: &DagEdge) -> StoreResult<()> {
    conn.execute(
        "INSERT INTO dag_edges (edge_id, graph_id, from_node_id, to_node_id, edge_type)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            e.edge_id,
            e.graph_id,
            e.from_node_id,
            e.to_node_id,
            e.edge_type
        ],
    )?;
    Ok(())
}

/// Get all nodes in a graph that are ready to run (no blocking dependencies).
///
/// A node is ready if:
/// - Its status is 'pending'
/// - All nodes it depends on via 'blocks' edges have status 'complete'
///
/// # Errors
///
/// Returns an error if the database query fails.
pub fn get_ready_nodes(conn: &Connection, graph_id: &str) -> StoreResult<Vec<DagNode>> {
    let mut stmt = conn.prepare(
        r"
        SELECT n.node_id, n.graph_id, n.label, n.status, n.task_id, n.created_at, n.completed_at
        FROM dag_nodes n
        WHERE n.graph_id = ?1
          AND n.status = 'pending'
          AND NOT EXISTS (
            SELECT 1 FROM dag_edges e
            JOIN dag_nodes dep ON e.from_node_id = dep.node_id
            WHERE e.to_node_id = n.node_id
              AND e.edge_type = 'blocks'
              AND dep.status != 'complete'
          )
        ORDER BY n.created_at ASC
        ",
    )?;

    let nodes = stmt.query_map([graph_id], |row| {
        Ok(DagNode {
            node_id: row.get(0)?,
            graph_id: row.get(1)?,
            label: row.get(2)?,
            status: row.get(3)?,
            task_id: row.get(4)?,
            created_at: row.get(5)?,
            completed_at: row.get(6)?,
        })
    })?;

    let mut result = Vec::new();
    for node in nodes {
        result.push(node?);
    }
    Ok(result)
}

/// Update a node's status and optionally its completion time.
///
/// # Errors
///
/// Returns an error if the database write fails or the node does not exist.
pub fn update_node_status(
    conn: &Connection,
    node_id: &str,
    status: &str,
    completed_at: Option<i64>,
) -> StoreResult<()> {
    let rows_changed = if let Some(completed_at) = completed_at {
        conn.execute(
            "UPDATE dag_nodes SET status = ?1, completed_at = ?2 WHERE node_id = ?3",
            rusqlite::params![status, completed_at, node_id],
        )?
    } else {
        conn.execute(
            "UPDATE dag_nodes SET status = ?1 WHERE node_id = ?2",
            rusqlite::params![status, node_id],
        )?
    };
    if rows_changed == 0 {
        return Err(super::StoreError::NotFound("dag_node"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r"
            CREATE TABLE dag_graphs (
                graph_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'open' CHECK(status IN ('open', 'complete', 'failed')),
                created_at INTEGER NOT NULL
            );
            CREATE TABLE dag_nodes (
                node_id TEXT PRIMARY KEY,
                graph_id TEXT NOT NULL REFERENCES dag_graphs(graph_id),
                label TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'ready', 'running', 'complete', 'failed')),
                task_id TEXT,
                created_at INTEGER NOT NULL,
                completed_at INTEGER
            );
            CREATE TABLE dag_edges (
                edge_id TEXT PRIMARY KEY,
                graph_id TEXT NOT NULL REFERENCES dag_graphs(graph_id),
                from_node_id TEXT NOT NULL REFERENCES dag_nodes(node_id),
                to_node_id TEXT NOT NULL REFERENCES dag_nodes(node_id),
                edge_type TEXT NOT NULL DEFAULT 'blocks' CHECK(edge_type IN ('blocks', 'informs'))
            );
            CREATE INDEX idx_dag_nodes_graph ON dag_nodes(graph_id);
            CREATE INDEX idx_dag_edges_to ON dag_edges(to_node_id);
            ",
        )
        .unwrap();
        conn
    }

    #[test]
    fn linear_chain_only_first_ready() {
        let conn = setup();
        let graph_id = "graph-1";
        let now = 1000i64;

        // Create graph
        let graph = DagGraph {
            graph_id: graph_id.to_string(),
            name: "Linear A->B->C".to_string(),
            status: "open".to_string(),
            created_at: now,
        };
        create_graph(&conn, &graph).unwrap();

        // Create nodes
        for (node_id, label) in &[("n-a", "A"), ("n-b", "B"), ("n-c", "C")] {
            let node = DagNode {
                node_id: node_id.to_string(),
                graph_id: graph_id.to_string(),
                label: label.to_string(),
                status: "pending".to_string(),
                task_id: None,
                created_at: now,
                completed_at: None,
            };
            add_node(&conn, &node).unwrap();
        }

        // Create edges: A -> B -> C
        let edges = vec![("e-ab", "n-a", "n-b"), ("e-bc", "n-b", "n-c")];
        for (edge_id, from, to) in edges {
            let edge = DagEdge {
                edge_id: edge_id.to_string(),
                graph_id: graph_id.to_string(),
                from_node_id: from.to_string(),
                to_node_id: to.to_string(),
                edge_type: "blocks".to_string(),
            };
            add_edge(&conn, &edge).unwrap();
        }

        // Only A should be ready
        let ready = get_ready_nodes(&conn, graph_id).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].node_id, "n-a");
    }

    #[test]
    fn complete_first_node_second_ready() {
        let conn = setup();
        let graph_id = "graph-2";
        let now = 1000i64;

        // Create graph, nodes, and edges
        let graph = DagGraph {
            graph_id: graph_id.to_string(),
            name: "Linear A->B->C".to_string(),
            status: "open".to_string(),
            created_at: now,
        };
        create_graph(&conn, &graph).unwrap();

        for (node_id, label) in &[("n-a", "A"), ("n-b", "B"), ("n-c", "C")] {
            let node = DagNode {
                node_id: node_id.to_string(),
                graph_id: graph_id.to_string(),
                label: label.to_string(),
                status: "pending".to_string(),
                task_id: None,
                created_at: now,
                completed_at: None,
            };
            add_node(&conn, &node).unwrap();
        }

        let edges = vec![("e-ab", "n-a", "n-b"), ("e-bc", "n-b", "n-c")];
        for (edge_id, from, to) in edges {
            let edge = DagEdge {
                edge_id: edge_id.to_string(),
                graph_id: graph_id.to_string(),
                from_node_id: from.to_string(),
                to_node_id: to.to_string(),
                edge_type: "blocks".to_string(),
            };
            add_edge(&conn, &edge).unwrap();
        }

        // Complete A
        update_node_status(&conn, "n-a", "complete", Some(now + 100)).unwrap();

        // Now B should be ready
        let ready = get_ready_nodes(&conn, graph_id).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].node_id, "n-b");
    }

    #[test]
    fn fan_out_both_ready_after_source() {
        let conn = setup();
        let graph_id = "graph-3";
        let now = 1000i64;

        // Create graph
        let graph = DagGraph {
            graph_id: graph_id.to_string(),
            name: "Fan-out A -> B, A -> C".to_string(),
            status: "open".to_string(),
            created_at: now,
        };
        create_graph(&conn, &graph).unwrap();

        // Create nodes A, B, C
        for (node_id, label) in &[("n-a", "A"), ("n-b", "B"), ("n-c", "C")] {
            let node = DagNode {
                node_id: node_id.to_string(),
                graph_id: graph_id.to_string(),
                label: label.to_string(),
                status: "pending".to_string(),
                task_id: None,
                created_at: now,
                completed_at: None,
            };
            add_node(&conn, &node).unwrap();
        }

        // Create edges: A -> B, A -> C
        let edges = vec![("e-ab", "n-a", "n-b"), ("e-ac", "n-a", "n-c")];
        for (edge_id, from, to) in edges {
            let edge = DagEdge {
                edge_id: edge_id.to_string(),
                graph_id: graph_id.to_string(),
                from_node_id: from.to_string(),
                to_node_id: to.to_string(),
                edge_type: "blocks".to_string(),
            };
            add_edge(&conn, &edge).unwrap();
        }

        // Only A is ready
        let ready = get_ready_nodes(&conn, graph_id).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].node_id, "n-a");

        // Complete A
        update_node_status(&conn, "n-a", "complete", Some(now + 50)).unwrap();

        // Now B and C should both be ready
        let ready = get_ready_nodes(&conn, graph_id).unwrap();
        assert_eq!(ready.len(), 2);
        let ids: Vec<&str> = ready.iter().map(|n| n.node_id.as_str()).collect();
        assert!(ids.contains(&"n-b"));
        assert!(ids.contains(&"n-c"));
    }

    #[test]
    fn fan_in_c_pending_until_both_sources_complete() {
        let conn = setup();
        let graph_id = "graph-4";
        let now = 1000i64;

        // Create graph
        let graph = DagGraph {
            graph_id: graph_id.to_string(),
            name: "Fan-in A -> C, B -> C".to_string(),
            status: "open".to_string(),
            created_at: now,
        };
        create_graph(&conn, &graph).unwrap();

        // Create nodes A, B, C
        for (node_id, label) in &[("n-a", "A"), ("n-b", "B"), ("n-c", "C")] {
            let node = DagNode {
                node_id: node_id.to_string(),
                graph_id: graph_id.to_string(),
                label: label.to_string(),
                status: "pending".to_string(),
                task_id: None,
                created_at: now,
                completed_at: None,
            };
            add_node(&conn, &node).unwrap();
        }

        // Create edges: A -> C, B -> C
        let edges = vec![("e-ac", "n-a", "n-c"), ("e-bc", "n-b", "n-c")];
        for (edge_id, from, to) in edges {
            let edge = DagEdge {
                edge_id: edge_id.to_string(),
                graph_id: graph_id.to_string(),
                from_node_id: from.to_string(),
                to_node_id: to.to_string(),
                edge_type: "blocks".to_string(),
            };
            add_edge(&conn, &edge).unwrap();
        }

        // A and B are ready
        let ready = get_ready_nodes(&conn, graph_id).unwrap();
        assert_eq!(ready.len(), 2);

        // Complete only A
        update_node_status(&conn, "n-a", "complete", Some(now + 50)).unwrap();

        // C should still be pending (B not complete)
        let ready = get_ready_nodes(&conn, graph_id).unwrap();
        let ids: Vec<&str> = ready.iter().map(|n| n.node_id.as_str()).collect();
        assert!(!ids.contains(&"n-c"));
        assert!(ids.contains(&"n-b"));

        // Complete B
        update_node_status(&conn, "n-b", "complete", Some(now + 60)).unwrap();

        // Now C should be ready
        let ready = get_ready_nodes(&conn, graph_id).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].node_id, "n-c");
    }
}
