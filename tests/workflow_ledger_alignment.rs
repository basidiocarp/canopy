//! Tests for handoff `#141d`: Canopy Workflow Ledger Alignment
//!
//! Covers:
//! - Task `workflow_id` and `phase_id` round-trip
//! - `DependsOn` relationship kind round-trip
//! - Handoff semantic context fields (`goal`, `next_steps`, `stop_reason`) round-trip

use canopy::models::{
    AgentRegistration, AgentStatus, HandoffType, TaskRelationshipKind, TaskRelationshipRole,
};
use canopy::store::{HandoffTiming, Store, TaskCreationOptions};
use tempfile::tempdir;

fn register_agent(store: &Store, agent_id: &str) -> AgentRegistration {
    let agent = AgentRegistration {
        agent_id: agent_id.to_string(),
        host_id: "host-1".to_string(),
        host_type: "claude".to_string(),
        host_instance: "local".to_string(),
        model: "claude-sonnet".to_string(),
        project_root: "/tmp/proj".to_string(),
        worktree_id: "wt-1".to_string(),
        status: AgentStatus::Idle,
        current_task_id: None,
        heartbeat_at: None,
        capabilities: Vec::new(),
        role: None,
    };
    store.register_agent(&agent).expect("register agent")
}

// --- Step 1: workflow_id and phase_id on tasks ---

#[test]
fn task_workflow_id_and_phase_id_round_trip() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let options = TaskCreationOptions {
        workflow_id: Some("wf-abc-123".to_string()),
        phase_id: Some("phase-impl".to_string()),
        ..TaskCreationOptions::default()
    };

    let task = store
        .create_task_with_options(
            "implement feature X",
            Some("part of workflow abc"),
            "operator",
            "/tmp/proj",
            &options,
        )
        .expect("create task with workflow linkage");

    assert_eq!(task.workflow_id.as_deref(), Some("wf-abc-123"));
    assert_eq!(task.phase_id.as_deref(), Some("phase-impl"));

    // Round-trip via get_task
    let loaded = store.get_task(&task.task_id).expect("get task");
    assert_eq!(loaded.workflow_id.as_deref(), Some("wf-abc-123"));
    assert_eq!(loaded.phase_id.as_deref(), Some("phase-impl"));

    // Surface via workflow context
    let context = store
        .get_task_workflow_context(&task.task_id)
        .expect("get workflow context");
    assert_eq!(context.workflow_id.as_deref(), Some("wf-abc-123"));
    assert_eq!(context.phase_id.as_deref(), Some("phase-impl"));
}

#[test]
fn task_without_workflow_linkage_has_none_fields() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let task = store
        .create_task("standalone task", None, "operator", "/tmp/proj", None)
        .expect("create task");

    assert!(task.workflow_id.is_none());
    assert!(task.phase_id.is_none());
}

// --- Step 2: DependsOn relationship ---

#[test]
fn depends_on_relationship_round_trips() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let task_a = store
        .create_task("task A", None, "operator", "/tmp/proj", None)
        .expect("create task A");
    let task_b = store
        .create_task("task B", None, "operator", "/tmp/proj", None)
        .expect("create task B");

    // task_a depends on task_b
    let relationship = store
        .add_task_relationship(
            &task_a.task_id,
            &task_b.task_id,
            TaskRelationshipKind::DependsOn,
            "operator",
        )
        .expect("create depends_on relationship");

    assert_eq!(relationship.kind, TaskRelationshipKind::DependsOn);
    assert_eq!(relationship.source_task_id, task_a.task_id);
    assert_eq!(relationship.target_task_id, task_b.task_id);

    // Load back via list_task_relationships
    let rels = store
        .list_task_relationships(Some(&task_a.task_id))
        .expect("list relationships");
    assert!(
        rels.iter()
            .any(|r| r.kind == TaskRelationshipKind::DependsOn),
        "DependsOn relationship not found in list"
    );

    // Verify directional roles via list_related_tasks
    let related = store
        .list_related_tasks(&task_a.task_id)
        .expect("list related tasks");
    let dep_rel = related
        .iter()
        .find(|r| r.related_task_id == task_b.task_id)
        .expect("should find task B as related to task A");
    assert_eq!(dep_rel.relationship_role, TaskRelationshipRole::DependsOn);

    // From the other side: task_b is a dependency_of task_a
    let related_b = store
        .list_related_tasks(&task_b.task_id)
        .expect("list related tasks for B");
    let dep_of_rel = related_b
        .iter()
        .find(|r| r.related_task_id == task_a.task_id)
        .expect("should find task A as related to task B");
    assert_eq!(
        dep_of_rel.relationship_role,
        TaskRelationshipRole::DependencyOf
    );
}

// --- Step 3: Semantic handoff context ---

#[test]
fn handoff_semantic_context_fields_round_trip() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let from_agent = register_agent(&store, "agent-from");
    let to_agent = register_agent(&store, "agent-to");

    let task = store
        .create_task("coordinate handoff", None, "operator", "/tmp/proj", None)
        .expect("create task");

    let handoff = store
        .create_handoff_with_context(
            &task.task_id,
            &from_agent.agent_id,
            &to_agent.agent_id,
            HandoffType::RequestReview,
            "please review implementation",
            Some("review src/lib.rs"),
            Some("ship feature X by end of sprint"),
            Some("review the diff and approve or request changes"),
            Some("needs_review"),
            HandoffTiming::default(),
        )
        .expect("create handoff with semantic context");

    // Verify round-trip on created handoff
    assert_eq!(
        handoff.goal.as_deref(),
        Some("ship feature X by end of sprint")
    );
    assert_eq!(
        handoff.next_steps.as_deref(),
        Some("review the diff and approve or request changes")
    );
    assert_eq!(handoff.stop_reason.as_deref(), Some("needs_review"));

    // Round-trip via get_handoff
    let loaded = store.get_handoff(&handoff.handoff_id).expect("get handoff");
    assert_eq!(loaded.goal, handoff.goal);
    assert_eq!(loaded.next_steps, handoff.next_steps);
    assert_eq!(loaded.stop_reason, handoff.stop_reason);

    // Round-trip via list_handoffs
    let all_handoffs = store
        .list_handoffs(Some(&task.task_id))
        .expect("list handoffs");
    assert_eq!(all_handoffs.len(), 1);
    let listed = &all_handoffs[0];
    assert_eq!(
        listed.goal.as_deref(),
        Some("ship feature X by end of sprint")
    );
    assert_eq!(
        listed.next_steps.as_deref(),
        Some("review the diff and approve or request changes")
    );
    assert_eq!(listed.stop_reason.as_deref(), Some("needs_review"));
}

#[test]
fn handoff_without_semantic_context_has_none_fields() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let from_agent = register_agent(&store, "agent-a");
    let to_agent = register_agent(&store, "agent-b");

    let task = store
        .create_task("simple handoff task", None, "operator", "/tmp/proj", None)
        .expect("create task");

    let handoff = store
        .create_handoff(
            &task.task_id,
            &from_agent.agent_id,
            &to_agent.agent_id,
            HandoffType::RequestReview,
            "standard handoff",
            None,
            HandoffTiming::default(),
        )
        .expect("create handoff");

    assert!(handoff.goal.is_none());
    assert!(handoff.next_steps.is_none());
    assert!(handoff.stop_reason.is_none());
}
