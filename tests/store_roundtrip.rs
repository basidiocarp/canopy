use canopy::models::{
    AgentRegistration, AgentStatus, CouncilMessageType, EvidenceSourceKind, HandoffStatus,
    HandoffType, TaskEventType, TaskStatus, VerificationState,
};
use canopy::store::Store;
use tempfile::tempdir;

#[test]
fn store_roundtrip_covers_agents_tasks_and_council_messages() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let agent = AgentRegistration {
        agent_id: "codex-1".to_string(),
        host_id: "codex-local".to_string(),
        host_type: "codex".to_string(),
        host_instance: "local".to_string(),
        model: "gpt-5.4".to_string(),
        project_root: "/tmp/project".to_string(),
        worktree_id: "wt-1".to_string(),
        status: AgentStatus::Idle,
        current_task_id: None,
        heartbeat_at: None,
    };
    let reviewer = AgentRegistration {
        agent_id: "claude-1".to_string(),
        host_id: "claude-local".to_string(),
        host_type: "claude".to_string(),
        host_instance: "local".to_string(),
        model: "opus".to_string(),
        project_root: "/tmp/project".to_string(),
        worktree_id: "wt-2".to_string(),
        status: AgentStatus::Idle,
        current_task_id: None,
        heartbeat_at: None,
    };

    store.register_agent(&agent).expect("register agent");
    store
        .register_agent(&reviewer)
        .expect("register reviewer agent");
    let agents = store.list_agents().expect("list agents");
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[0].agent_id, reviewer.agent_id);
    assert_eq!(agents[0].status, AgentStatus::Idle);
    assert!(agents[0].heartbeat_at.is_some());
    assert_eq!(agents[1].agent_id, agent.agent_id);
    assert_eq!(agents[1].status, AgentStatus::Idle);
    assert!(agents[1].heartbeat_at.is_some());

    let task = store
        .create_task(
            "Investigate recall mismatch",
            Some("trace the scoring attribution"),
            "operator",
            "/tmp/project",
        )
        .expect("create task");
    assert_eq!(task.status, TaskStatus::Open);
    assert!(task.owner_agent_id.is_none());

    let initial_events = store
        .list_task_events(&task.task_id)
        .expect("list initial events");
    assert_eq!(initial_events.len(), 1);
    assert_eq!(initial_events[0].event_type, TaskEventType::Created);
    assert_eq!(initial_events[0].to_status, TaskStatus::Open);
    assert_eq!(initial_events[0].actor, "operator");

    let assigned = store
        .assign_task(
            &task.task_id,
            &agent.agent_id,
            "operator",
            Some("best available host for implementation"),
        )
        .expect("assign task");
    assert_eq!(assigned.status, TaskStatus::Assigned);
    assert_eq!(
        assigned.owner_agent_id.as_deref(),
        Some(agent.agent_id.as_str())
    );

    let heartbeat = store
        .heartbeat_agent(
            &agent.agent_id,
            AgentStatus::InProgress,
            Some(&task.task_id),
        )
        .expect("heartbeat agent");
    assert_eq!(heartbeat.status, AgentStatus::InProgress);
    assert_eq!(
        heartbeat.current_task_id.as_deref(),
        Some(task.task_id.as_str())
    );
    assert!(heartbeat.heartbeat_at.is_some());

    let tasks = store.list_tasks().expect("list tasks");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0], assigned);
    let shown = store.get_task(&task.task_id).expect("show task");
    assert_eq!(shown, assigned);

    let handoff = store
        .create_handoff(
            &task.task_id,
            &agent.agent_id,
            &reviewer.agent_id,
            HandoffType::RequestReview,
            "ask for an independent contract review",
            Some("confirm the evidence boundary"),
        )
        .expect("create handoff");
    assert_eq!(handoff.status, HandoffStatus::Open);
    assert_eq!(handoff.handoff_type, HandoffType::RequestReview);

    let resolved = store
        .resolve_handoff(&handoff.handoff_id, HandoffStatus::Accepted)
        .expect("resolve handoff");
    assert_eq!(resolved.status, HandoffStatus::Accepted);

    let handoffs = store
        .list_handoffs(Some(&task.task_id))
        .expect("list handoffs");
    assert_eq!(handoffs, vec![resolved]);

    let transferred = store.get_task(&task.task_id).expect("reload task");
    assert_eq!(
        transferred.owner_agent_id.as_deref(),
        Some(reviewer.agent_id.as_str())
    );
    assert_eq!(transferred.status, TaskStatus::Assigned);
    assert_eq!(transferred.verification_state, VerificationState::Unknown);

    let message = store
        .add_council_message(
            &task.task_id,
            &agent.agent_id,
            CouncilMessageType::Proposal,
            "use scoped session ids as direct evidence refs",
        )
        .expect("create council message");
    assert_eq!(message.task_id, task.task_id);
    assert_eq!(message.message_type, CouncilMessageType::Proposal);

    let evidence = store
        .add_evidence(
            &task.task_id,
            EvidenceSourceKind::HyphaeSession,
            "session:01KMSCANOPY",
            "hyphae session",
            Some("session backing the review"),
            Some(&handoff.handoff_id),
        )
        .expect("create evidence");

    let messages = store
        .list_council_messages(&task.task_id)
        .expect("list council messages");
    assert_eq!(messages, vec![message]);

    let evidence_refs = store.list_evidence(&task.task_id).expect("list evidence");
    assert_eq!(evidence_refs, vec![evidence]);

    let review_required = store
        .update_task_status(
            &task.task_id,
            TaskStatus::ReviewRequired,
            "operator",
            Some(VerificationState::Pending),
            None,
            None,
        )
        .expect("mark review required");
    assert_eq!(review_required.status, TaskStatus::ReviewRequired);
    assert_eq!(
        review_required.verification_state,
        VerificationState::Pending
    );
    assert_eq!(review_required.verified_by.as_deref(), Some("operator"));
    assert!(review_required.verified_at.is_some());
    assert!(review_required.closed_at.is_none());

    let blocked = store
        .update_task_status(
            &task.task_id,
            TaskStatus::Blocked,
            "operator",
            None,
            Some("waiting on a second opinion"),
            None,
        )
        .expect("mark blocked");
    assert_eq!(blocked.status, TaskStatus::Blocked);
    assert_eq!(
        blocked.blocked_reason.as_deref(),
        Some("waiting on a second opinion")
    );

    let completed = store
        .update_task_status(
            &task.task_id,
            TaskStatus::Completed,
            "claude-1",
            Some(VerificationState::Passed),
            None,
            Some("review completed and accepted"),
        )
        .expect("complete task");
    assert_eq!(completed.status, TaskStatus::Completed);
    assert_eq!(completed.verification_state, VerificationState::Passed);
    assert_eq!(completed.closed_by.as_deref(), Some("claude-1"));
    assert_eq!(
        completed.closure_summary.as_deref(),
        Some("review completed and accepted")
    );
    assert!(completed.closed_at.is_some());
    assert!(completed.blocked_reason.is_none());

    let events = store
        .list_task_events(&task.task_id)
        .expect("list task events");
    assert_eq!(events.len(), 6);
    assert_eq!(events[1].event_type, TaskEventType::Assigned);
    assert_eq!(events[1].to_status, TaskStatus::Assigned);
    assert_eq!(
        events[1].owner_agent_id.as_deref(),
        Some(agent.agent_id.as_str())
    );
    assert_eq!(events[2].event_type, TaskEventType::OwnershipTransferred);
    assert_eq!(
        events[2].owner_agent_id.as_deref(),
        Some(reviewer.agent_id.as_str())
    );
    assert_eq!(events[3].event_type, TaskEventType::StatusChanged);
    assert_eq!(events[3].to_status, TaskStatus::ReviewRequired);
    assert_eq!(
        events[3].verification_state,
        Some(VerificationState::Pending)
    );
    assert_eq!(events[4].event_type, TaskEventType::StatusChanged);
    assert_eq!(events[4].to_status, TaskStatus::Blocked);
    assert_eq!(
        events[4].note.as_deref(),
        Some("waiting on a second opinion")
    );
    assert_eq!(events[5].event_type, TaskEventType::StatusChanged);
    assert_eq!(events[5].to_status, TaskStatus::Completed);
    assert_eq!(
        events[5].verification_state,
        Some(VerificationState::Passed)
    );
    assert_eq!(
        events[5].note.as_deref(),
        Some("review completed and accepted")
    );

    let refreshed_agents = store.list_agents().expect("list agents after transfer");
    let refreshed_reviewer = refreshed_agents
        .iter()
        .find(|candidate| candidate.agent_id == reviewer.agent_id)
        .expect("reviewer agent present");
    assert_eq!(refreshed_reviewer.current_task_id.as_deref(), None);
    assert_eq!(refreshed_reviewer.status, AgentStatus::Idle);
    assert!(refreshed_reviewer.heartbeat_at.is_some());

    let refreshed_owner = refreshed_agents
        .iter()
        .find(|candidate| candidate.agent_id == agent.agent_id)
        .expect("owner agent present");
    assert!(refreshed_owner.current_task_id.is_none());
    assert_eq!(refreshed_owner.status, AgentStatus::Idle);
}
