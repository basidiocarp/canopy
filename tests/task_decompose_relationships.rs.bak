use canopy::models::TaskRelationshipKind;
use canopy::store::Store;
use canopy::tools::task::tool_task_decompose;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn tool_task_decompose_persists_relationship_on_dependency() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    // Create parent task
    let parent = store
        .create_task("Parent task", None, "operator", "/tmp/proj", None)
        .expect("create parent");

    // Decompose into two subtasks where the second depends on the first
    let args = json!({
        "parent_task_id": parent.task_id,
        "subtasks": [
            { "title": "First subtask" },
            { "title": "Second subtask", "depends_on_index": 0 }
        ]
    });

    let result = tool_task_decompose(&store, "operator", &args);
    assert!(!result.is_error, "decompose should succeed");

    // Parse the result to get the subtask IDs
    let content = result.content[0].text.clone();
    let decompose_result: serde_json::Value =
        serde_json::from_str(&content).expect("parse decompose result");

    let subtasks = decompose_result["subtasks"]
        .as_array()
        .expect("subtasks array");
    assert_eq!(subtasks.len(), 2, "should have created 2 subtasks");

    let first_task_id = subtasks[0]["task_id"].as_str().expect("first task_id");
    let second_task_id = subtasks[1]["task_id"].as_str().expect("second task_id");

    // Verify the Blocks relationship was persisted
    let relationships = store
        .list_task_relationships(Some(first_task_id))
        .expect("list relationships");

    let blocks_relationship = relationships
        .iter()
        .find(|rel| {
            rel.source_task_id == first_task_id
                && rel.target_task_id == second_task_id
                && rel.kind == TaskRelationshipKind::Blocks
        })
        .expect("Blocks relationship should exist");

    assert_eq!(blocks_relationship.source_task_id, first_task_id);
    assert_eq!(blocks_relationship.target_task_id, second_task_id);
    assert_eq!(blocks_relationship.kind, TaskRelationshipKind::Blocks);

    // Also verify from the second task's perspective
    let second_relationships = store
        .list_task_relationships(Some(second_task_id))
        .expect("list relationships for second task");

    let reverse_relationship = second_relationships
        .iter()
        .find(|rel| rel.source_task_id == first_task_id && rel.target_task_id == second_task_id)
        .expect("should find relationship from second task's query");

    assert_eq!(reverse_relationship.kind, TaskRelationshipKind::Blocks);
}

#[test]
fn tool_task_decompose_with_no_dependencies() {
    // Verify that decompose works correctly when there are no dependencies
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let parent = store
        .create_task("Parent task", None, "operator", "/tmp/proj", None)
        .expect("create parent");

    let args = json!({
        "parent_task_id": parent.task_id,
        "subtasks": [
            { "title": "Independent subtask 1" },
            { "title": "Independent subtask 2" }
        ]
    });

    let result = tool_task_decompose(&store, "operator", &args);
    assert!(!result.is_error, "decompose should succeed");

    let content = result.content[0].text.clone();
    let decompose_result: serde_json::Value =
        serde_json::from_str(&content).expect("parse decompose result");

    let subtasks = decompose_result["subtasks"]
        .as_array()
        .expect("subtasks array");
    assert_eq!(subtasks.len(), 2, "should have created 2 subtasks");

    // Verify no Blocks relationships were created
    let all_relationships = store
        .list_task_relationships(None)
        .expect("list all relationships");

    let blocks_rels: Vec<_> = all_relationships
        .iter()
        .filter(|rel| rel.kind == TaskRelationshipKind::Blocks)
        .collect();

    assert_eq!(blocks_rels.len(), 0, "no Blocks relationships should exist");
}
