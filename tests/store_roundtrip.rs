use canopy::models::{
    AgentRegistration, AgentStatus, CouncilMessageType, EvidenceSourceKind, HandoffStatus,
    HandoffType, OperatorActionKind, TaskEventType, TaskRelationshipRole, TaskStatus,
    VerificationState,
};
use canopy::store::{
    EvidenceLinkRefs, HandoffOperatorActionInput, HandoffTiming, Store, TaskOperatorActionInput,
    TaskStatusUpdate,
};
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
    assert!(!task.created_at.is_empty());
    assert!(!task.updated_at.is_empty());
    assert!(
        store
            .heartbeat_agent(&agent.agent_id, AgentStatus::InProgress, None)
            .is_err()
    );
    assert!(
        store
            .heartbeat_agent(
                &agent.agent_id,
                AgentStatus::InProgress,
                Some(&task.task_id)
            )
            .is_err()
    );

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
    assert!(
        store
            .register_agent(&AgentRegistration {
                status: AgentStatus::Idle,
                current_task_id: Some(task.task_id.clone()),
                ..agent.clone()
            })
            .is_err()
    );
    assert!(
        store
            .register_agent(&AgentRegistration {
                status: AgentStatus::InProgress,
                current_task_id: None,
                ..reviewer.clone()
            })
            .is_err()
    );
    let second_task = store
        .create_task("Second task", None, "operator", "/tmp/project")
        .expect("create second task");
    assert!(
        store
            .assign_task(&second_task.task_id, &agent.agent_id, "operator", None)
            .is_err()
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
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0], assigned);
    assert_eq!(tasks[1].task_id, second_task.task_id);
    let shown = store.get_task(&task.task_id).expect("show task");
    assert_eq!(shown, assigned);

    let handoff = store
        .create_handoff(
            &task.task_id,
            &agent.agent_id,
            &reviewer.agent_id,
            HandoffType::TransferOwnership,
            "ask for an independent contract review",
            Some("confirm the evidence boundary"),
            HandoffTiming::default(),
        )
        .expect("create handoff");
    assert_eq!(handoff.status, HandoffStatus::Open);
    assert_eq!(handoff.handoff_type, HandoffType::TransferOwnership);
    assert!(!handoff.created_at.is_empty());
    assert!(!handoff.updated_at.is_empty());
    assert!(handoff.resolved_at.is_none());

    let resolved = store
        .resolve_handoff(&handoff.handoff_id, HandoffStatus::Accepted, "claude-1")
        .expect("resolve handoff");
    assert_eq!(resolved.status, HandoffStatus::Accepted);
    assert!(resolved.resolved_at.is_some());
    assert!(
        store
            .resolve_handoff(&handoff.handoff_id, HandoffStatus::Completed, "claude-1")
            .is_err()
    );

    let handoffs = store
        .list_handoffs(Some(&task.task_id))
        .expect("list handoffs");
    assert_eq!(handoffs, vec![resolved]);
    let assignments = store
        .list_task_assignments(Some(&task.task_id))
        .expect("list assignments");
    assert_eq!(assignments.len(), 2);
    assert_eq!(assignments[0].assigned_to, agent.agent_id);
    assert_eq!(assignments[1].assigned_to, reviewer.agent_id);
    assert_eq!(assignments[1].assigned_by, "claude-1");

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
            EvidenceLinkRefs {
                related_handoff_id: Some(&handoff.handoff_id),
                ..EvidenceLinkRefs::default()
            },
        )
        .expect("create evidence");
    assert_eq!(
        evidence.related_session_id.as_deref(),
        Some("session:01KMSCANOPY")
    );
    assert!(evidence.related_memory_query.is_none());

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
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Pending),
                ..TaskStatusUpdate::default()
            },
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
            TaskStatusUpdate {
                blocked_reason: Some("waiting on a second opinion"),
                ..TaskStatusUpdate::default()
            },
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
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Passed),
                closure_summary: Some("review completed and accepted"),
                ..TaskStatusUpdate::default()
            },
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
    assert_eq!(events.len(), 7);
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
    assert_eq!(events[3].event_type, TaskEventType::HandoffUpdated);
    assert_eq!(events[3].to_status, TaskStatus::Assigned);
    let handoff_note = events[3].note.as_deref().expect("handoff note");
    assert!(handoff_note.starts_with("handoff_action=resolve; handoff_id="));
    assert!(handoff_note.ends_with("; status:open->accepted"));
    assert_eq!(events[4].event_type, TaskEventType::StatusChanged);
    assert_eq!(events[4].to_status, TaskStatus::ReviewRequired);
    assert_eq!(
        events[4].verification_state,
        Some(VerificationState::Pending)
    );
    assert_eq!(events[5].event_type, TaskEventType::StatusChanged);
    assert_eq!(events[5].to_status, TaskStatus::Blocked);
    assert_eq!(
        events[5].note.as_deref(),
        Some("waiting on a second opinion")
    );
    assert_eq!(events[6].event_type, TaskEventType::StatusChanged);
    assert_eq!(events[6].to_status, TaskStatus::Completed);
    assert_eq!(
        events[6].verification_state,
        Some(VerificationState::Passed)
    );
    assert_eq!(
        events[6].note.as_deref(),
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

    let task_heartbeats = store
        .list_task_heartbeats(&task.task_id, 20)
        .expect("list task heartbeats");
    assert!(
        task_heartbeats
            .iter()
            .any(|heartbeat| heartbeat.agent_id == reviewer.agent_id)
    );
    assert!(
        task_heartbeats
            .iter()
            .any(|heartbeat| heartbeat.agent_id == agent.agent_id)
    );
}

#[test]
fn task_creation_actions_create_artifacts_and_record_history() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    for agent in [
        AgentRegistration {
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
        },
        AgentRegistration {
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
        },
    ] {
        store.register_agent(&agent).expect("register agent");
    }

    let task = store
        .create_task("Operator coordination", None, "operator", "/tmp/project")
        .expect("create task");
    let _ = store
        .assign_task(&task.task_id, "codex-1", "operator", Some("initial owner"))
        .expect("assign task");

    let _ = store
        .apply_task_operator_action(
            &task.task_id,
            OperatorActionKind::CreateHandoff,
            "operator",
            TaskOperatorActionInput {
                from_agent_id: Some("codex-1"),
                to_agent_id: Some("claude-1"),
                handoff_type: Some(HandoffType::RequestReview),
                handoff_summary: Some("review the coordination patch"),
                requested_action: Some("confirm the runtime contract"),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect("create handoff action");

    let created_handoff = store
        .list_handoffs(Some(&task.task_id))
        .expect("list handoffs")
        .into_iter()
        .next()
        .expect("created handoff");

    let _ = store
        .apply_task_operator_action(
            &task.task_id,
            OperatorActionKind::PostCouncilMessage,
            "operator",
            TaskOperatorActionInput {
                author_agent_id: Some("codex-1"),
                message_type: Some(CouncilMessageType::Status),
                message_body: Some("Ready for operator follow-through."),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect("post council message");

    let _ = store
        .apply_task_operator_action(
            &task.task_id,
            OperatorActionKind::AttachEvidence,
            "operator",
            TaskOperatorActionInput {
                evidence_source_kind: Some(EvidenceSourceKind::HyphaeSession),
                evidence_source_ref: Some("ses_456"),
                evidence_label: Some("Hyphae session"),
                evidence_summary: Some("Linked implementation session"),
                related_handoff_id: Some(created_handoff.handoff_id.as_str()),
                related_session_id: Some("ses_456"),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect("attach evidence");

    let _ = store
        .apply_task_operator_action(
            &task.task_id,
            OperatorActionKind::CreateFollowUpTask,
            "operator",
            TaskOperatorActionInput {
                follow_up_title: Some("Close the follow-up queue"),
                follow_up_description: Some("Track the remaining operator work."),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect("create follow-up task");

    let tasks = store.list_tasks().expect("list tasks");
    assert_eq!(tasks.len(), 2);
    assert!(
        tasks
            .iter()
            .any(|item| item.title == "Close the follow-up queue")
    );
    let follow_up_task = tasks
        .iter()
        .find(|item| item.title == "Close the follow-up queue")
        .expect("follow-up task");
    let blocker_task = store
        .create_task("Lifecycle blocker", None, "operator", "/tmp/project")
        .expect("create blocker task");

    let _ = store
        .apply_task_operator_action(
            &task.task_id,
            OperatorActionKind::LinkTaskDependency,
            "operator",
            TaskOperatorActionInput {
                related_task_id: Some(&blocker_task.task_id),
                relationship_role: Some(TaskRelationshipRole::BlockedBy),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect("link dependency");

    let relationships = store
        .list_task_relationships(Some(&task.task_id))
        .expect("list task relationships");
    assert_eq!(relationships.len(), 2);
    assert!(relationships.iter().any(|relationship| {
        relationship.kind.to_string() == "follow_up"
            && relationship.source_task_id == task.task_id
            && relationship.target_task_id == follow_up_task.task_id
    }));
    assert!(relationships.iter().any(|relationship| {
        relationship.kind.to_string() == "blocks"
            && relationship.source_task_id == blocker_task.task_id
            && relationship.target_task_id == task.task_id
    }));

    let related_tasks = store
        .list_related_tasks(&task.task_id)
        .expect("list related tasks");
    assert!(related_tasks.iter().any(|related| {
        related.related_task_id == follow_up_task.task_id
            && related.relationship_role == TaskRelationshipRole::FollowUpChild
    }));
    assert!(related_tasks.iter().any(|related| {
        related.related_task_id == blocker_task.task_id
            && related.relationship_role == TaskRelationshipRole::BlockedBy
    }));

    let messages = store
        .list_council_messages(&task.task_id)
        .expect("list council messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message_type, CouncilMessageType::Status);

    let evidence = store.list_evidence(&task.task_id).expect("list evidence");
    assert_eq!(evidence.len(), 1);
    assert_eq!(
        evidence[0].related_handoff_id.as_deref(),
        Some(created_handoff.handoff_id.as_str())
    );

    let events = store
        .list_task_events(&task.task_id)
        .expect("list task events");
    assert!(events.iter().any(|event| {
        event.event_type == TaskEventType::HandoffCreated
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("handoff_id="))
    }));
    assert!(events.iter().any(|event| {
        event.event_type == TaskEventType::CouncilMessagePosted
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("message_id="))
    }));
    assert!(events.iter().any(|event| {
        event.event_type == TaskEventType::EvidenceAttached
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("evidence_id="))
    }));
    assert!(events.iter().any(|event| {
        event.event_type == TaskEventType::FollowUpTaskCreated
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("follow_up_task_id="))
    }));
    assert!(events.iter().any(|event| {
        event.event_type == TaskEventType::RelationshipUpdated
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("kind=follow_up"))
    }));
    assert!(events.iter().any(|event| {
        event.event_type == TaskEventType::RelationshipUpdated
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("kind=blocks"))
    }));
}

#[test]
fn task_creation_actions_reject_terminal_tasks() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let task = store
        .create_task("Closed coordination task", None, "operator", "/tmp/project")
        .expect("create task");
    let _ = store
        .update_task_status(
            &task.task_id,
            TaskStatus::Completed,
            "operator",
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Passed),
                closure_summary: Some("done"),
                ..TaskStatusUpdate::default()
            },
        )
        .expect("complete task");

    let error = store
        .apply_task_operator_action(
            &task.task_id,
            OperatorActionKind::CreateFollowUpTask,
            "operator",
            TaskOperatorActionInput {
                follow_up_title: Some("Should not be allowed"),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect_err("reject follow-up creation on terminal task");

    assert!(
        error
            .to_string()
            .contains("operator action create_follow_up_task is not valid for terminal tasks")
    );
}

#[test]
fn graph_operator_actions_update_relationships_and_status() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let parent = store
        .create_task("Coordinate release", None, "operator", "/tmp/project")
        .expect("create parent");
    let blocker = store
        .create_task("Fix blocker", None, "operator", "/tmp/project")
        .expect("create blocker");

    store
        .apply_task_operator_action(
            &parent.task_id,
            OperatorActionKind::LinkTaskDependency,
            "operator",
            TaskOperatorActionInput {
                related_task_id: Some(&blocker.task_id),
                relationship_role: Some(TaskRelationshipRole::BlockedBy),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect("link dependency");
    store
        .apply_task_operator_action(
            &parent.task_id,
            OperatorActionKind::BlockTask,
            "operator",
            TaskOperatorActionInput {
                blocked_reason: Some("waiting on dependency"),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect("block task");

    let reopen_error = store
        .apply_task_operator_action(
            &parent.task_id,
            OperatorActionKind::ReopenBlockedTaskWhenUnblocked,
            "operator",
            TaskOperatorActionInput::default(),
        )
        .expect_err("reject reopen while blockers remain");
    assert!(reopen_error.to_string().contains("no remaining blockers"));

    store
        .apply_task_operator_action(
            &parent.task_id,
            OperatorActionKind::ResolveDependency,
            "operator",
            TaskOperatorActionInput {
                related_task_id: Some(&blocker.task_id),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect("resolve dependency");
    assert!(
        store
            .list_related_tasks(&parent.task_id)
            .expect("list related tasks after dependency resolution")
            .into_iter()
            .all(|related| related.relationship_role != TaskRelationshipRole::BlockedBy)
    );

    let reopened = store
        .apply_task_operator_action(
            &parent.task_id,
            OperatorActionKind::ReopenBlockedTaskWhenUnblocked,
            "operator",
            TaskOperatorActionInput::default(),
        )
        .expect("reopen blocked task");
    assert_eq!(reopened.status, TaskStatus::Open);
    assert!(reopened.blocked_reason.is_none());

    store
        .apply_task_operator_action(
            &parent.task_id,
            OperatorActionKind::CreateFollowUpTask,
            "operator",
            TaskOperatorActionInput {
                follow_up_title: Some("Follow up A"),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect("create first follow-up");
    let follow_up_a = store
        .list_tasks()
        .expect("list tasks")
        .into_iter()
        .find(|task| task.title == "Follow up A")
        .expect("follow-up A");

    store
        .apply_task_operator_action(
            &parent.task_id,
            OperatorActionKind::PromoteFollowUp,
            "operator",
            TaskOperatorActionInput {
                related_task_id: Some(&follow_up_a.task_id),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect("promote follow-up");
    assert!(
        store
            .list_related_tasks(&parent.task_id)
            .expect("list related tasks after promotion")
            .into_iter()
            .all(|related| related.related_task_id != follow_up_a.task_id)
    );

    store
        .apply_task_operator_action(
            &parent.task_id,
            OperatorActionKind::CreateFollowUpTask,
            "operator",
            TaskOperatorActionInput {
                follow_up_title: Some("Follow up B"),
                ..TaskOperatorActionInput::default()
            },
        )
        .expect("create second follow-up");
    let follow_up_b = store
        .list_tasks()
        .expect("list tasks")
        .into_iter()
        .find(|task| task.title == "Follow up B")
        .expect("follow-up B");
    store
        .update_task_status(
            &follow_up_b.task_id,
            TaskStatus::Completed,
            "operator",
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Passed),
                closure_summary: Some("done"),
                ..TaskStatusUpdate::default()
            },
        )
        .expect("complete second follow-up");

    store
        .apply_task_operator_action(
            &parent.task_id,
            OperatorActionKind::CloseFollowUpChain,
            "operator",
            TaskOperatorActionInput::default(),
        )
        .expect("close follow-up chain");
    assert!(
        store
            .list_related_tasks(&parent.task_id)
            .expect("list related tasks after close")
            .into_iter()
            .all(|related| related.relationship_role != TaskRelationshipRole::FollowUpChild)
    );

    let parent_events = store
        .list_task_events(&parent.task_id)
        .expect("list parent events");
    assert!(parent_events.iter().any(|event| {
        event.event_type == TaskEventType::RelationshipUpdated
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("action=resolve_dependency"))
    }));
    assert!(parent_events.iter().any(|event| {
        event.event_type == TaskEventType::RelationshipUpdated
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("action=promote_follow_up"))
    }));
    assert!(parent_events.iter().any(|event| {
        event.event_type == TaskEventType::RelationshipUpdated
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("action=close_follow_up_chain"))
    }));
}

#[test]
fn handoff_operator_actions_cover_resolution_paths() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    for (agent_id, host_id, host_type, host_instance, model) in [
        ("codex-1", "codex-local", "codex", "local", "gpt-5.4"),
        ("claude-1", "claude-local", "claude", "local", "opus"),
        ("codex-2", "codex-remote", "codex", "remote", "gpt-5.4-mini"),
    ] {
        store
            .register_agent(&AgentRegistration {
                agent_id: agent_id.to_string(),
                host_id: host_id.to_string(),
                host_type: host_type.to_string(),
                host_instance: host_instance.to_string(),
                model: model.to_string(),
                project_root: "/tmp/project".to_string(),
                worktree_id: format!("wt-{agent_id}"),
                status: AgentStatus::Idle,
                current_task_id: None,
                heartbeat_at: None,
            })
            .expect("register agent");
    }

    let transfer_task = store
        .create_task("Transfer owner", None, "operator", "/tmp/project")
        .expect("create transfer task");
    store
        .assign_task(&transfer_task.task_id, "codex-1", "operator", None)
        .expect("assign transfer task");
    let transfer_handoff = store
        .create_handoff(
            &transfer_task.task_id,
            "codex-1",
            "claude-1",
            HandoffType::TransferOwnership,
            "pass task to reviewer",
            None,
            HandoffTiming::default(),
        )
        .expect("create transfer handoff");

    let accepted = store
        .apply_handoff_operator_action(
            &transfer_handoff.handoff_id,
            OperatorActionKind::AcceptHandoff,
            "operator",
            HandoffOperatorActionInput::default(),
        )
        .expect("accept transfer handoff");
    assert_eq!(accepted.status, HandoffStatus::Accepted);
    assert_eq!(
        store
            .get_task(&transfer_task.task_id)
            .expect("reload transfer task")
            .owner_agent_id
            .as_deref(),
        Some("claude-1")
    );

    let rejected_task = store
        .create_task("Reject help", None, "operator", "/tmp/project")
        .expect("create rejected task");
    let rejected_handoff = store
        .create_handoff(
            &rejected_task.task_id,
            "codex-1",
            "claude-1",
            HandoffType::RequestHelp,
            "cannot pick this up",
            None,
            HandoffTiming::default(),
        )
        .expect("create rejected handoff");
    assert_eq!(
        store
            .apply_handoff_operator_action(
                &rejected_handoff.handoff_id,
                OperatorActionKind::RejectHandoff,
                "operator",
                HandoffOperatorActionInput::default(),
            )
            .expect("reject handoff")
            .status,
        HandoffStatus::Rejected
    );
    let rejected_task_after = store
        .get_task(&rejected_task.task_id)
        .expect("reload rejected task");
    assert_eq!(rejected_task_after.status, TaskStatus::Open);
    assert!(rejected_task_after.owner_agent_id.is_none());

    let cancelled_task = store
        .create_task("Cancel request", None, "operator", "/tmp/project")
        .expect("create cancelled task");
    let cancelled_handoff = store
        .create_handoff(
            &cancelled_task.task_id,
            "codex-1",
            "claude-1",
            HandoffType::RequestReview,
            "review no longer needed",
            None,
            HandoffTiming::default(),
        )
        .expect("create cancelled handoff");
    assert_eq!(
        store
            .apply_handoff_operator_action(
                &cancelled_handoff.handoff_id,
                OperatorActionKind::CancelHandoff,
                "operator",
                HandoffOperatorActionInput::default(),
            )
            .expect("cancel handoff")
            .status,
        HandoffStatus::Cancelled
    );
    let cancelled_task_after = store
        .get_task(&cancelled_task.task_id)
        .expect("reload cancelled task");
    assert_eq!(cancelled_task_after.status, TaskStatus::Open);
    assert!(cancelled_task_after.owner_agent_id.is_none());

    let completed_task = store
        .create_task("Complete review", None, "operator", "/tmp/project")
        .expect("create completed task");
    let completed_handoff = store
        .create_handoff(
            &completed_task.task_id,
            "codex-2",
            "claude-1",
            HandoffType::RequestReview,
            "review finished externally",
            None,
            HandoffTiming::default(),
        )
        .expect("create completed handoff");
    assert_eq!(
        store
            .apply_handoff_operator_action(
                &completed_handoff.handoff_id,
                OperatorActionKind::CompleteHandoff,
                "operator",
                HandoffOperatorActionInput::default(),
            )
            .expect("complete handoff")
            .status,
        HandoffStatus::Completed
    );
    let completed_task_after = store
        .get_task(&completed_task.task_id)
        .expect("reload completed task");
    assert_eq!(completed_task_after.status, TaskStatus::Open);
    assert!(completed_task_after.owner_agent_id.is_none());

    let expired_task = store
        .create_task("Expire review", None, "operator", "/tmp/project")
        .expect("create expired task");
    let expired_handoff = store
        .create_handoff(
            &expired_task.task_id,
            "codex-2",
            "claude-1",
            HandoffType::RequestReview,
            "review window elapsed",
            None,
            HandoffTiming {
                expires_at: Some("2020-01-01T00:00:00Z"),
                ..HandoffTiming::default()
            },
        )
        .expect("create expired handoff");
    assert_eq!(
        store
            .apply_handoff_operator_action(
                &expired_handoff.handoff_id,
                OperatorActionKind::ExpireHandoff,
                "operator",
                HandoffOperatorActionInput::default(),
            )
            .expect("expire handoff")
            .status,
        HandoffStatus::Expired
    );
    let expired_handoff_for_follow_up = store
        .create_handoff(
            &expired_task.task_id,
            "codex-2",
            "claude-1",
            HandoffType::RequestHelp,
            "stale follow-up should fail",
            None,
            HandoffTiming {
                expires_at: Some("2020-01-01T00:00:00Z"),
                ..HandoffTiming::default()
            },
        )
        .expect("create expired handoff for follow-up");
    assert!(
        store
            .apply_handoff_operator_action(
                &expired_handoff_for_follow_up.handoff_id,
                OperatorActionKind::FollowUpHandoff,
                "operator",
                HandoffOperatorActionInput::default(),
            )
            .is_err()
    );

    let history = store
        .list_task_events(&completed_task.task_id)
        .expect("list task history");
    assert!(
        history.iter().any(|event| {
            event.event_type == TaskEventType::HandoffUpdated
                && event
                    .note
                    .as_deref()
                    .is_some_and(|note| note.contains("status:open->completed"))
        }),
        "expected handoff completion to be recorded in task history"
    );
}
