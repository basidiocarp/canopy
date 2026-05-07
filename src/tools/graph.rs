use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct HandoffGraph {
    pub schema_version: String,
    pub graph_id: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub entry_points: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub handoff_slug: String,
    pub owning_repo: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    pub wave: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub relationship: GraphRelationship,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphRelationship {
    DependsOn,
    Produces,
    Reviews,
}

#[derive(Debug, Serialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

/// Validate a handoff graph file and print results.
///
/// # Errors
///
/// Returns an error if the file cannot be read, parsed, or validation fails.
pub fn validate_graph_file(path: &Path, json: bool) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let graph: HandoffGraph =
        serde_json::from_str(&content).with_context(|| "failed to parse JSON graph file")?;

    let result = validate_graph(&graph);

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if result.valid {
        println!("VALID");
    } else {
        println!("INVALID");
        for error in &result.errors {
            println!("  - {error}");
        }
    }

    if result.valid {
        Ok(())
    } else {
        anyhow::bail!("graph validation failed")
    }
}

fn validate_graph(graph: &HandoffGraph) -> ValidationResult {
    let mut errors = Vec::new();

    // Build node ID set
    let node_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();

    // Check edge references
    for edge in &graph.edges {
        if !node_ids.contains(edge.from.as_str()) {
            errors.push(format!("edge from unknown node: {}", edge.from));
        }
        if !node_ids.contains(edge.to.as_str()) {
            errors.push(format!("edge to unknown node: {}", edge.to));
        }
    }

    // Check entry points reference real nodes
    for ep in &graph.entry_points {
        if !node_ids.contains(ep.as_str()) {
            errors.push(format!("entry_point references unknown node: {ep}"));
        }
    }

    // Check entry points have no incoming depends_on edges
    let nodes_with_incoming_depends_on: HashSet<&str> = graph
        .edges
        .iter()
        .filter(|e| e.relationship == GraphRelationship::DependsOn)
        .map(|e| e.to.as_str())
        .collect();

    for ep in &graph.entry_points {
        if nodes_with_incoming_depends_on.contains(ep.as_str()) {
            errors.push(format!("entry_point {ep} has incoming depends_on edges"));
        }
    }

    // Cycle detection (Kahn's algorithm on depends_on edges only).
    // Guarded by errors.is_empty() so that dangling edge references (which would
    // confuse the in-degree counts) are reported first. Fix structural errors, then
    // re-run to surface any cycles that remain.
    if errors.is_empty() {
        if let Some(cycle_err) = detect_cycle(graph) {
            errors.push(cycle_err);
        }
    }

    ValidationResult {
        valid: errors.is_empty(),
        errors,
    }
}

fn detect_cycle(graph: &HandoffGraph) -> Option<String> {
    let node_ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let index: HashMap<&str, usize> = node_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();
    let n = node_ids.len();

    let mut in_degree = vec![0usize; n];
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];

    for edge in &graph.edges {
        if edge.relationship != GraphRelationship::DependsOn {
            continue;
        }
        if let (Some(&from_idx), Some(&to_idx)) =
            (index.get(edge.from.as_str()), index.get(edge.to.as_str()))
        {
            adj[from_idx].push(to_idx);
            in_degree[to_idx] += 1;
        }
    }

    let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut visited = 0;

    while let Some(node) = queue.pop_front() {
        visited += 1;
        for &next in &adj[node] {
            in_degree[next] -= 1;
            if in_degree[next] == 0 {
                queue.push_back(next);
            }
        }
    }

    if visited < n {
        Some("cycle detected in depends_on edges".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_graph(
        nodes: Vec<(&str, &str, &str)>,
        edges: Vec<(&str, &str, &str)>,
        entry_points: Vec<&str>,
    ) -> HandoffGraph {
        HandoffGraph {
            schema_version: "1.0".to_string(),
            graph_id: "test".to_string(),
            nodes: nodes
                .into_iter()
                .map(|(id, slug, repo)| GraphNode {
                    id: id.to_string(),
                    handoff_slug: slug.to_string(),
                    owning_repo: repo.to_string(),
                    allowed_tools: vec![],
                    wave: None,
                })
                .collect(),
            edges: edges
                .into_iter()
                .map(|(from, to, rel)| GraphEdge {
                    from: from.to_string(),
                    to: to.to_string(),
                    relationship: match rel {
                        "depends_on" => GraphRelationship::DependsOn,
                        "produces" => GraphRelationship::Produces,
                        _ => GraphRelationship::Reviews,
                    },
                })
                .collect(),
            entry_points: entry_points.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn valid_linear_graph_passes() {
        let graph = make_graph(
            vec![("a", "slug-a", "repo"), ("b", "slug-b", "repo")],
            vec![("a", "b", "depends_on")],
            vec!["a"],
        );
        let result = validate_graph(&graph);
        assert!(result.valid, "{:?}", result.errors);
    }

    #[test]
    fn cycle_is_detected() {
        let graph = make_graph(
            vec![("a", "s", "r"), ("b", "s", "r")],
            vec![("a", "b", "depends_on"), ("b", "a", "depends_on")],
            vec![],
        );
        let result = validate_graph(&graph);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("cycle")));
    }

    #[test]
    fn missing_edge_target_is_flagged() {
        let graph = make_graph(
            vec![("a", "s", "r")],
            vec![("a", "missing", "depends_on")],
            vec!["a"],
        );
        let result = validate_graph(&graph);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("missing")));
    }

    #[test]
    fn entry_point_with_incoming_depends_on_is_flagged() {
        let graph = make_graph(
            vec![("a", "s", "r"), ("b", "s", "r")],
            vec![("a", "b", "depends_on")],
            vec!["b"], // b has an incoming edge, wrong
        );
        let result = validate_graph(&graph);
        assert!(!result.valid);
    }

    #[test]
    fn empty_graph_is_valid() {
        // An empty graph (no nodes, no edges, no entry points) is a degenerate but
        // structurally valid DAG. The caller is responsible for ensuring completeness.
        let graph = make_graph(vec![], vec![], vec![]);
        let result = validate_graph(&graph);
        assert!(result.valid, "{:?}", result.errors);
    }
}
