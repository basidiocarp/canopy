#![allow(clippy::too_many_lines)]

use canopy::models::{
    AgentRegistration, AgentRole, AgentStatus, CouncilMessageType, EvidenceSourceKind,
    HandoffStatus, HandoffType, OperatorActionKind, TaskAction, TaskEventType,
    TaskRelationshipRole, TaskStatus, VerificationState,
};
use canopy::store::{
    EvidenceLinkRefs, HandoffOperatorActionInput, HandoffTiming, Store, TaskCreationOptions,
    TaskDeadlineUpdate, TaskStatusUpdate,
};
use rusqlite::Connection;
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
        capabilities: Vec::new(),
        role: None,
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
        capabilities: Vec::new(),
        role: None,
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
            None,
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
        .create_task("Second task", None, "operator", "/tmp/project", None)
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
        .resolve_handoff_with_actor(
            &handoff.handoff_id,
            HandoffStatus::Accepted,
            "claude-1",
            Some("claude-1"),
        )
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
    assert_eq!(evidence.schema_version, "1.0");
    assert_eq!(
        evidence.related_session_id.as_deref(),
        Some("session:01KMSCANOPY")
    );
    assert!(evidence.related_memory_query.is_none());

    let messages = store
        .list_council_messages(&task.task_id)
        .expect("list council messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message_id, message.message_id);
    assert_eq!(messages[0].task_id, message.task_id);
    assert_eq!(messages[0].author_agent_id, message.author_agent_id);
    assert_eq!(messages[0].message_type, message.message_type);
    assert_eq!(messages[0].body, message.body);
    assert!(messages[0].created_at.is_some());

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

    let resumed = store
        .update_task_status(
            &task.task_id,
            TaskStatus::Assigned,
            "claude-1",
            TaskStatusUpdate::default(),
        )
        .expect("resume task from blocked");
    assert_eq!(resumed.status, TaskStatus::Assigned);
    assert!(resumed.blocked_reason.is_none());

    let in_progress = store
        .update_task_status(
            &task.task_id,
            TaskStatus::InProgress,
            "claude-1",
            TaskStatusUpdate::default(),
        )
        .expect("move task back into progress");
    assert_eq!(in_progress.status, TaskStatus::InProgress);

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
    assert_eq!(events.len(), 9);
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
    assert_eq!(events[6].to_status, TaskStatus::Assigned);
    assert_eq!(events[7].event_type, TaskEventType::StatusChanged);
    assert_eq!(events[7].to_status, TaskStatus::InProgress);
    assert_eq!(events[8].event_type, TaskEventType::StatusChanged);
    assert_eq!(events[8].to_status, TaskStatus::Completed);
    assert_eq!(
        events[8].verification_state,
        Some(VerificationState::Passed)
    );
    assert_eq!(
        events[8].note.as_deref(),
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
fn store_open_enables_wal_and_busy_timeout() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    let _store = Store::open(&db_path).expect("open store");

    let conn = Connection::open(&db_path).expect("open db");
    let journal_mode: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .expect("read journal_mode");
    let busy_timeout: i64 = conn
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .expect("read busy_timeout");

    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    assert_eq!(busy_timeout, 5000);
}

#[test]
fn update_task_status_rejects_invalid_terminal_transition() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let task = store
        .create_task("Cancelled task", None, "operator", "/tmp/project", None)
        .expect("create task");

    store
        .update_task_status(
            &task.task_id,
            TaskStatus::Cancelled,
            "operator",
            TaskStatusUpdate {
                closure_summary: Some("cancelled"),
                ..TaskStatusUpdate::default()
            },
        )
        .expect("cancel task");

    let error = store
        .update_task_status(
            &task.task_id,
            TaskStatus::InProgress,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect_err("cancelled task should not go directly to in progress");

    assert!(
        error
            .to_string()
            .contains("cannot transition from cancelled to in_progress")
    );
}

#[test]
fn update_task_status_allows_reopen_from_closed_to_open() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let task = store
        .create_task("Closed task", None, "operator", "/tmp/project", None)
        .expect("create task");

    store
        .update_task_status(
            &task.task_id,
            TaskStatus::InProgress,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect("start task");
    store
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
    let closed = store
        .update_task_status(
            &task.task_id,
            TaskStatus::Closed,
            "operator",
            TaskStatusUpdate {
                closure_summary: Some("closed"),
                ..TaskStatusUpdate::default()
            },
        )
        .expect("close task");
    assert_eq!(closed.status, TaskStatus::Closed);

    let reopened = store
        .update_task_status(
            &task.task_id,
            TaskStatus::Open,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect("reopen task");
    assert_eq!(reopened.status, TaskStatus::Open);
}

#[test]
fn assign_task_enforces_required_role_when_both_task_and_agent_define_it() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    store
        .register_agent(&AgentRegistration {
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
            capabilities: Vec::new(),
            role: Some(AgentRole::Implementer),
        })
        .expect("register implementer");

    store
        .register_agent(&AgentRegistration {
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
            capabilities: Vec::new(),
            role: Some(AgentRole::Validator),
        })
        .expect("register validator");

    let task = store
        .create_task(
            "Validate task role routing",
            None,
            "operator",
            "/tmp/project",
            Some(AgentRole::Validator),
        )
        .expect("create validator task");
    assert_eq!(task.required_role, Some(AgentRole::Validator));

    let error = store
        .assign_task(&task.task_id, "codex-1", "operator", None)
        .expect_err("reject mismatched assignee role");
    assert!(
        error
            .to_string()
            .contains("task requires validator role, agent has implementer")
    );

    let assigned = store
        .assign_task(&task.task_id, "claude-1", "operator", None)
        .expect("assign validator task");
    assert_eq!(assigned.owner_agent_id.as_deref(), Some("claude-1"));
    assert_eq!(assigned.required_role, Some(AgentRole::Validator));
}

#[test]
fn assign_task_remains_backward_compatible_when_role_is_missing_on_one_side() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    store
        .register_agent(&AgentRegistration {
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
            capabilities: Vec::new(),
            role: None,
        })
        .expect("register legacy agent");

    store
        .register_agent(&AgentRegistration {
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
            capabilities: Vec::new(),
            role: Some(AgentRole::Validator),
        })
        .expect("register validator");

    let legacy_task = store
        .create_task("Legacy open task", None, "operator", "/tmp/project", None)
        .expect("create legacy task");
    let validator_task = store
        .create_task(
            "Validator task with legacy assignee",
            None,
            "operator",
            "/tmp/project",
            Some(AgentRole::Validator),
        )
        .expect("create validator task");

    let legacy_agent_target = store
        .assign_task(&validator_task.task_id, "codex-1", "operator", None)
        .expect("allow missing agent role");
    assert_eq!(
        legacy_agent_target.owner_agent_id.as_deref(),
        Some("codex-1")
    );

    let validator_assignment = store
        .assign_task(&legacy_task.task_id, "claude-1", "operator", None)
        .expect("allow missing task role");
    assert_eq!(
        validator_assignment.owner_agent_id.as_deref(),
        Some("claude-1")
    );

    let reloaded_legacy_agent = store
        .list_agents()
        .expect("list agents")
        .into_iter()
        .find(|agent| agent.agent_id == "codex-1")
        .expect("legacy agent still present");
    assert_eq!(reloaded_legacy_agent.role, None);

    let reloaded_legacy_task = store
        .get_task(&legacy_task.task_id)
        .expect("reload legacy task");
    assert_eq!(reloaded_legacy_task.required_role, None);
}

#[test]
fn assign_and_claim_task_enforce_required_capabilities_when_both_sides_declare_them() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    store
        .register_agent(&AgentRegistration {
            agent_id: "codex-1".to_string(),
            host_id: "codex-local".to_string(),
            host_type: "codex".to_string(),
            host_instance: "local".to_string(),
            model: "gpt-5.4".to_string(),
            project_root: "/tmp/project".to_string(),
            worktree_id: "wt-1".to_string(),
            role: Some(AgentRole::Implementer),
            capabilities: vec!["rust".to_string(), "hyphae".to_string()],
            status: AgentStatus::Idle,
            current_task_id: None,
            heartbeat_at: None,
        })
        .expect("register matching agent");
    store
        .register_agent(&AgentRegistration {
            agent_id: "codex-2".to_string(),
            host_id: "codex-remote".to_string(),
            host_type: "codex".to_string(),
            host_instance: "remote".to_string(),
            model: "gpt-5.4-mini".to_string(),
            project_root: "/tmp/project".to_string(),
            worktree_id: "wt-2".to_string(),
            role: Some(AgentRole::Implementer),
            capabilities: vec!["rust".to_string()],
            status: AgentStatus::Idle,
            current_task_id: None,
            heartbeat_at: None,
        })
        .expect("register incomplete agent");

    let task = store
        .create_task_with_options(
            "Capability-gated implementation",
            None,
            "operator",
            "/tmp/project",
            &TaskCreationOptions {
                required_role: Some(AgentRole::Implementer),
                required_capabilities: vec!["rust".to_string(), "hyphae".to_string()],
                auto_review: false,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create capability-gated task");

    let error = store
        .assign_task(&task.task_id, "codex-2", "operator", None)
        .expect_err("reject missing capability on assign");
    assert!(
        error
            .to_string()
            .contains("agent missing capabilities: hyphae")
    );

    let claimed = store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::Claim {
                acting_agent_id: "codex-1",
                note: None,
            },
        )
        .expect("claim task with matching capabilities");
    assert_eq!(claimed.owner_agent_id.as_deref(), Some("codex-1"));
}

#[test]
fn assign_task_capabilities_stay_backward_compatible_for_empty_lists_and_case_sensitive() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    store
        .register_agent(&AgentRegistration {
            agent_id: "codex-1".to_string(),
            host_id: "codex-local".to_string(),
            host_type: "codex".to_string(),
            host_instance: "local".to_string(),
            model: "gpt-5.4".to_string(),
            project_root: "/tmp/project".to_string(),
            worktree_id: "wt-1".to_string(),
            role: Some(AgentRole::Implementer),
            capabilities: Vec::new(),
            status: AgentStatus::Idle,
            current_task_id: None,
            heartbeat_at: None,
        })
        .expect("register empty-capability agent");
    store
        .register_agent(&AgentRegistration {
            agent_id: "codex-2".to_string(),
            host_id: "codex-remote".to_string(),
            host_type: "codex".to_string(),
            host_instance: "remote".to_string(),
            model: "gpt-5.4-mini".to_string(),
            project_root: "/tmp/project".to_string(),
            worktree_id: "wt-2".to_string(),
            role: Some(AgentRole::Implementer),
            capabilities: vec!["rust".to_string()],
            status: AgentStatus::Idle,
            current_task_id: None,
            heartbeat_at: None,
        })
        .expect("register rust agent");
    store
        .register_agent(&AgentRegistration {
            agent_id: "codex-3".to_string(),
            host_id: "codex-third".to_string(),
            host_type: "codex".to_string(),
            host_instance: "remote".to_string(),
            model: "gpt-5.4-mini".to_string(),
            project_root: "/tmp/project".to_string(),
            worktree_id: "wt-3".to_string(),
            role: Some(AgentRole::Implementer),
            capabilities: vec!["rust".to_string()],
            status: AgentStatus::Idle,
            current_task_id: None,
            heartbeat_at: None,
        })
        .expect("register second rust agent");

    let no_requirement_task = store
        .create_task_with_options(
            "No capability requirement",
            None,
            "operator",
            "/tmp/project",
            &TaskCreationOptions {
                required_role: Some(AgentRole::Implementer),
                ..TaskCreationOptions::default()
            },
        )
        .expect("create unscoped task");
    store
        .assign_task(&no_requirement_task.task_id, "codex-2", "operator", None)
        .expect("allow empty required capability list");

    let no_agent_capability_task = store
        .create_task_with_options(
            "Agent missing capability list",
            None,
            "operator",
            "/tmp/project",
            &TaskCreationOptions {
                required_role: Some(AgentRole::Implementer),
                required_capabilities: vec!["rust".to_string()],
                auto_review: false,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create task with capability requirement");
    store
        .assign_task(
            &no_agent_capability_task.task_id,
            "codex-1",
            "operator",
            None,
        )
        .expect("allow empty agent capability list");

    let case_sensitive_task = store
        .create_task_with_options(
            "Case-sensitive task",
            None,
            "operator",
            "/tmp/project",
            &TaskCreationOptions {
                required_role: Some(AgentRole::Implementer),
                required_capabilities: vec!["Rust".to_string()],
                auto_review: false,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create case-sensitive task");
    let error = store
        .assign_task(&case_sensitive_task.task_id, "codex-3", "operator", None)
        .expect_err("reject capability mismatch with different casing");
    assert!(
        error
            .to_string()
            .contains("agent missing capabilities: Rust")
    );
}

#[test]
fn completed_review_handoff_creates_validator_review_siblings_for_auto_review_tasks() {
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
            capabilities: Vec::new(),
            role: Some(AgentRole::Implementer),
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
            capabilities: Vec::new(),
            role: Some(AgentRole::Validator),
        },
    ] {
        store.register_agent(&agent).expect("register agent");
    }

    let parent = store
        .create_task("Parent task", None, "operator", "/tmp/project", None)
        .expect("create parent");
    let implementation = store
        .create_subtask_with_options(
            &parent.task_id,
            "Implementation task",
            Some("Ship the runtime change"),
            "operator",
            &TaskCreationOptions {
                required_role: Some(AgentRole::Implementer),
                required_capabilities: vec!["rust".to_string(), "hyphae".to_string()],
                auto_review: true,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create implementation task");
    assert!(implementation.auto_review);

    let handoff = store
        .create_handoff(
            &implementation.task_id,
            "codex-1",
            "claude-1",
            HandoffType::RequestReview,
            "ready for structured review",
            None,
            HandoffTiming::default(),
        )
        .expect("create handoff");

    let resolved = store
        .resolve_handoff(&handoff.handoff_id, HandoffStatus::Completed, "operator")
        .expect("complete handoff");
    assert_eq!(resolved.status, HandoffStatus::Completed);

    let children = store
        .get_children(&parent.task_id)
        .expect("get parent children");
    assert_eq!(children.len(), 4);
    assert!(
        children
            .iter()
            .any(|task| task.task_id == implementation.task_id)
    );

    let review_tasks = store
        .list_tasks()
        .expect("list tasks")
        .into_iter()
        .filter(|task| {
            matches!(
                task.title.as_str(),
                "Spec review" | "Architecture audit" | "Quality check"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(review_tasks.len(), 3);
    for review_task in &review_tasks {
        assert_eq!(review_task.required_role, Some(AgentRole::Validator));
        assert!(!review_task.auto_review);
        assert_eq!(
            store
                .get_parent_id(&review_task.task_id)
                .expect("load review parent"),
            Some(parent.task_id.clone())
        );
        assert!(
            review_task
                .description
                .as_deref()
                .is_some_and(|description| description.contains(&implementation.task_id))
        );
    }

    let task_events = store
        .list_task_events(&implementation.task_id)
        .expect("list implementation task events");
    assert!(task_events.iter().any(|event| {
        event.event_type == TaskEventType::RelationshipUpdated
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("action=auto_review_subtasks"))
    }));
}

#[test]
fn completed_review_handoff_skips_auto_review_when_task_flag_is_disabled() {
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
            capabilities: Vec::new(),
            role: Some(AgentRole::Implementer),
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
            capabilities: Vec::new(),
            role: Some(AgentRole::Validator),
        },
    ] {
        store.register_agent(&agent).expect("register agent");
    }

    let task = store
        .create_task(
            "Implementation task",
            None,
            "operator",
            "/tmp/project",
            None,
        )
        .expect("create task");
    assert!(!task.auto_review);

    let handoff = store
        .create_handoff(
            &task.task_id,
            "codex-1",
            "claude-1",
            HandoffType::RequestReview,
            "review not auto-generated",
            None,
            HandoffTiming::default(),
        )
        .expect("create handoff");
    store
        .resolve_handoff(&handoff.handoff_id, HandoffStatus::Completed, "operator")
        .expect("complete handoff");

    let titles = store
        .list_tasks()
        .expect("list tasks")
        .into_iter()
        .map(|task| task.title)
        .collect::<Vec<_>>();
    assert_eq!(titles, vec!["Implementation task".to_string()]);
}

#[test]
fn store_requires_prior_execution_before_resume_task() {
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
        capabilities: Vec::new(),
        role: None,
    };

    store.register_agent(&agent).expect("register agent");
    let task = store
        .create_task("Resume execution", None, "operator", "/tmp/project", None)
        .expect("create task");
    let assigned = store
        .assign_task(&task.task_id, &agent.agent_id, "operator", None)
        .expect("assign task");
    assert_eq!(assigned.status, TaskStatus::Assigned);

    assert!(
        store
            .apply_task_operator_action(
                &task.task_id,
                "operator",
                TaskAction::Resume {
                    acting_agent_id: &agent.agent_id,
                    note: None
                },
            )
            .is_err()
    );

    let in_progress = store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::Start {
                acting_agent_id: &agent.agent_id,
                note: None,
            },
        )
        .expect("start task");
    assert_eq!(in_progress.status, TaskStatus::InProgress);

    let paused = store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::Pause {
                acting_agent_id: &agent.agent_id,
                note: None,
            },
        )
        .expect("pause task");
    assert_eq!(paused.status, TaskStatus::Assigned);

    let resumed = store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::Resume {
                acting_agent_id: &agent.agent_id,
                note: None,
            },
        )
        .expect("resume task");
    assert_eq!(resumed.status, TaskStatus::InProgress);

    let actions: Vec<_> = store
        .list_task_events(&task.task_id)
        .expect("list task events")
        .into_iter()
        .filter_map(|event| event.execution_action)
        .collect();
    assert!(actions.contains(&canopy::models::ExecutionActionKind::StartTask));
    assert!(actions.contains(&canopy::models::ExecutionActionKind::PauseTask));
    assert!(actions.contains(&canopy::models::ExecutionActionKind::ResumeTask));
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
            capabilities: Vec::new(),
            role: None,
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
            capabilities: Vec::new(),
            role: None,
        },
    ] {
        store.register_agent(&agent).expect("register agent");
    }

    let task = store
        .create_task(
            "Operator coordination",
            None,
            "operator",
            "/tmp/project",
            None,
        )
        .expect("create task");
    let _ = store
        .assign_task(&task.task_id, "codex-1", "operator", Some("initial owner"))
        .expect("assign task");

    let _ = store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::CreateHandoff {
                from_agent_id: "codex-1",
                to_agent_id: "claude-1",
                handoff_type: HandoffType::RequestReview,
                handoff_summary: "review the coordination patch",
                requested_action: Some("confirm the runtime contract"),
                due_at: None,
                expires_at: None,
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
            "operator",
            TaskAction::PostCouncilMessage {
                author_agent_id: "codex-1",
                message_type: CouncilMessageType::Status,
                message_body: "Ready for operator follow-through.",
            },
        )
        .expect("post council message");

    let _ = store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::AttachEvidence {
                source_kind: EvidenceSourceKind::HyphaeSession,
                source_ref: "ses_456",
                label: "Hyphae session",
                summary: Some("Linked implementation session"),
                related_handoff_id: Some(created_handoff.handoff_id.as_str()),
                related_session_id: Some("ses_456"),
                related_memory_query: None,
                related_symbol: None,
                related_file: None,
            },
        )
        .expect("attach evidence");

    let _ = store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::CreateFollowUp {
                title: "Close the follow-up queue",
                description: Some("Track the remaining operator work."),
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
    assert!(
        store
            .get_parent_id(&follow_up_task.task_id)
            .expect("follow-up parent id")
            .is_none(),
        "follow-up relationships should not become structural parent links"
    );
    assert!(
        store
            .get_children(&task.task_id)
            .expect("structural children")
            .is_empty(),
        "follow-up tasks should not count as structural children"
    );
    let blocker_task = store
        .create_task("Lifecycle blocker", None, "operator", "/tmp/project", None)
        .expect("create blocker task");

    let _ = store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::LinkDependency {
                related_task_id: &blocker_task.task_id,
                relationship_role: TaskRelationshipRole::BlockedBy,
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
    assert_eq!(evidence[0].schema_version, "1.0");
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
        .create_task(
            "Closed coordination task",
            None,
            "operator",
            "/tmp/project",
            None,
        )
        .expect("create task");
    let _ = store
        .update_task_status(
            &task.task_id,
            TaskStatus::InProgress,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect("start task");
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
            "operator",
            TaskAction::CreateFollowUp {
                title: "Should not be allowed",
                description: None,
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
fn subtasks_create_parent_relationships_and_enforce_single_parent() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let parent = store
        .create_task("Parent task", None, "operator", "/tmp/project", None)
        .expect("create parent");
    let other_parent = store
        .create_task("Other parent", None, "operator", "/tmp/project", None)
        .expect("create other parent");

    let child_a = store
        .create_subtask(&parent.task_id, "Child A", None, "operator", None)
        .expect("create child a");
    let child_b = store
        .create_subtask(
            &parent.task_id,
            "Child B",
            Some("verify output"),
            "operator",
            None,
        )
        .expect("create child b");

    let children = store.get_children(&parent.task_id).expect("get children");
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].task_id, child_a.task_id);
    assert_eq!(children[0].status, TaskStatus::Open);
    assert_eq!(children[1].task_id, child_b.task_id);
    assert_eq!(
        store
            .get_task(&child_a.task_id)
            .expect("reload child a")
            .parent_task_id
            .as_deref(),
        Some(parent.task_id.as_str())
    );
    assert_eq!(
        store
            .get_parent_id(&child_a.task_id)
            .expect("get parent id"),
        Some(parent.task_id.clone())
    );

    let parent_related = store
        .list_related_tasks(&parent.task_id)
        .expect("list parent related tasks");
    assert!(parent_related.iter().any(|related| {
        related.related_task_id == child_a.task_id
            && related.relationship_role == TaskRelationshipRole::Child
    }));

    let child_related = store
        .list_related_tasks(&child_a.task_id)
        .expect("list child related tasks");
    assert!(child_related.iter().any(|related| {
        related.related_task_id == parent.task_id
            && related.relationship_role == TaskRelationshipRole::Parent
    }));

    let second_parent_error = store
        .link_parent_task(&child_a.task_id, &other_parent.task_id, "operator")
        .expect_err("child should reject second parent");
    assert!(
        second_parent_error
            .to_string()
            .contains("task already has a parent")
    );

    let self_parent_error = store
        .link_parent_task(&parent.task_id, &parent.task_id, "operator")
        .expect_err("task should not parent itself");
    assert!(
        self_parent_error
            .to_string()
            .contains("task relationships must link two different tasks")
    );
}

#[test]
fn deleting_parent_does_not_delete_children() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");

    let (parent_id, child_id) = {
        let store = Store::open(&db_path).expect("open store");
        let parent = store
            .create_task("Delete parent", None, "operator", "/tmp/project", None)
            .expect("create parent");
        let child = store
            .create_subtask(&parent.task_id, "Keep child", None, "operator", None)
            .expect("create child");
        (parent.task_id, child.task_id)
    };

    let conn = Connection::open(&db_path).expect("open raw connection");
    conn.execute("PRAGMA foreign_keys = ON", [])
        .expect("enable foreign keys");
    conn.execute("DELETE FROM tasks WHERE task_id = ?1", [&parent_id])
        .expect("delete parent task");
    drop(conn);

    let store = Store::open(&db_path).expect("reopen store");
    let child = store
        .get_task(&child_id)
        .expect("child survives parent delete");
    assert_eq!(child.title, "Keep child");
    assert!(child.parent_task_id.is_none());
    assert!(
        store
            .get_parent_id(&child_id)
            .expect("load child parent after delete")
            .is_none()
    );
}

#[test]
fn review_operator_actions_record_decision_before_closeout() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    store
        .register_agent(&AgentRegistration {
            agent_id: "claude-1".to_string(),
            host_id: "claude-local".to_string(),
            host_type: "claude".to_string(),
            host_instance: "local".to_string(),
            model: "opus".to_string(),
            project_root: "/tmp/project".to_string(),
            worktree_id: "wt-1".to_string(),
            status: AgentStatus::Idle,
            current_task_id: None,
            heartbeat_at: None,
            capabilities: Vec::new(),
            role: None,
        })
        .expect("register reviewer");

    let task = store
        .create_task(
            "Close reviewed task",
            None,
            "operator",
            "/tmp/project",
            None,
        )
        .expect("create task");
    store
        .update_task_status(
            &task.task_id,
            TaskStatus::ReviewRequired,
            "operator",
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Pending),
                ..TaskStatusUpdate::default()
            },
        )
        .expect("move task into review");
    store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::AttachEvidence {
                source_kind: EvidenceSourceKind::ManualNote,
                source_ref: "review-note-1",
                label: "Operator note",
                summary: None,
                related_handoff_id: None,
                related_session_id: None,
                related_memory_query: None,
                related_symbol: None,
                related_file: None,
            },
        )
        .expect("attach evidence");

    let error = store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::Close {
                closure_summary: "premature closeout",
                note: None,
            },
        )
        .expect_err("reject closeout before a recorded decision");
    assert!(
        error
            .to_string()
            .contains("close_task requires a current-cycle decision context")
    );

    let review_task = store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::RecordDecision {
                author_agent_id: "claude-1",
                message_body: "Ship the reviewed task.",
            },
        )
        .expect("record decision");
    assert_eq!(review_task.status, TaskStatus::ReviewRequired);
    assert_eq!(review_task.verification_state, VerificationState::Pending);

    let completed_task = store
        .apply_task_operator_action(
            &task.task_id,
            "operator",
            TaskAction::Close {
                closure_summary: "review accepted and closed out",
                note: None,
            },
        )
        .expect("close task");
    assert_eq!(completed_task.status, TaskStatus::Completed);
    assert_eq!(completed_task.verification_state, VerificationState::Passed);

    let messages = store
        .list_council_messages(&task.task_id)
        .expect("list messages");
    assert!(
        messages
            .iter()
            .any(|message| message.message_type == CouncilMessageType::Decision)
    );

    let events = store
        .list_task_events(&task.task_id)
        .expect("list task events");
    assert!(events.iter().any(|event| {
        event.event_type == TaskEventType::CouncilMessagePosted
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("action=record_decision"))
    }));
    assert!(events.iter().any(|event| {
        event.event_type == TaskEventType::StatusChanged
            && event.to_status == TaskStatus::Completed
            && event
                .note
                .as_deref()
                .is_some_and(|note| note.contains("review accepted and closed out"))
    }));
}

#[test]
fn graph_operator_actions_update_relationships_and_status() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let parent = store
        .create_task("Coordinate release", None, "operator", "/tmp/project", None)
        .expect("create parent");
    let blocker = store
        .create_task("Fix blocker", None, "operator", "/tmp/project", None)
        .expect("create blocker");

    store
        .apply_task_operator_action(
            &parent.task_id,
            "operator",
            TaskAction::LinkDependency {
                related_task_id: &blocker.task_id,
                relationship_role: TaskRelationshipRole::BlockedBy,
            },
        )
        .expect("link dependency");
    store
        .apply_task_operator_action(
            &parent.task_id,
            "operator",
            TaskAction::Block {
                blocked_reason: "waiting on dependency",
                note: None,
            },
        )
        .expect("block task");

    let reopen_error = store
        .apply_task_operator_action(
            &parent.task_id,
            "operator",
            TaskAction::ReopenWhenUnblocked { note: None },
        )
        .expect_err("reject reopen while blockers remain");
    assert!(reopen_error.to_string().contains("no remaining blockers"));

    store
        .apply_task_operator_action(
            &parent.task_id,
            "operator",
            TaskAction::ResolveDependency {
                related_task_id: &blocker.task_id,
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
            "operator",
            TaskAction::ReopenWhenUnblocked { note: None },
        )
        .expect("reopen blocked task");
    assert_eq!(reopened.status, TaskStatus::Open);
    assert!(reopened.blocked_reason.is_none());

    store
        .apply_task_operator_action(
            &parent.task_id,
            "operator",
            TaskAction::CreateFollowUp {
                title: "Follow up A",
                description: None,
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
            "operator",
            TaskAction::PromoteFollowUp {
                related_task_id: &follow_up_a.task_id,
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
            "operator",
            TaskAction::CreateFollowUp {
                title: "Follow up B",
                description: None,
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
            TaskStatus::InProgress,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect("start second follow-up");
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
        .apply_task_operator_action(&parent.task_id, "operator", TaskAction::CloseFollowUpChain)
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
                capabilities: Vec::new(),
                role: None,
            })
            .expect("register agent");
    }

    let transfer_task = store
        .create_task("Transfer owner", None, "operator", "/tmp/project", None)
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
            HandoffOperatorActionInput {
                acting_agent_id: Some("claude-1"),
                ..HandoffOperatorActionInput::default()
            },
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
        .create_task("Reject help", None, "operator", "/tmp/project", None)
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
                HandoffOperatorActionInput {
                    acting_agent_id: Some("claude-1"),
                    ..HandoffOperatorActionInput::default()
                },
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
        .create_task("Cancel request", None, "operator", "/tmp/project", None)
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
        .create_task("Complete review", None, "operator", "/tmp/project", None)
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
        .create_task("Expire review", None, "operator", "/tmp/project", None)
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

#[test]
fn verification_required_tasks_need_script_evidence_before_completion() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let task = store
        .create_task_with_options(
            "Verified handoff step",
            None,
            "operator",
            "/tmp/project",
            &TaskCreationOptions {
                verification_required: true,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create verification-required task");
    store
        .update_task_status(
            &task.task_id,
            TaskStatus::InProgress,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect("start task");

    let error = store
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
        .expect_err("reject completion without script evidence");
    assert!(
        error
            .to_string()
            .contains("requires script verification evidence")
    );

    store
        .add_evidence(
            &task.task_id,
            EvidenceSourceKind::ScriptVerification,
            "/tmp/verify.sh",
            "Verification script",
            Some("script verification passed\n\nResults: 2 passed, 0 failed"),
            EvidenceLinkRefs::default(),
        )
        .expect("attach passing script evidence");

    let completed = store
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
        .expect("complete task after verification");
    assert_eq!(completed.status, TaskStatus::Completed);
}

#[test]
fn verified_parent_auto_completes_when_all_children_complete() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let parent = store
        .create_task_with_options(
            "Imported handoff",
            None,
            "operator",
            "/tmp/project",
            &TaskCreationOptions {
                verification_required: true,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create parent");
    store
        .add_evidence(
            &parent.task_id,
            EvidenceSourceKind::ScriptVerification,
            "/tmp/verify-parent.sh",
            "Verification script",
            Some("script verification passed\n\nResults: 1 passed, 0 failed"),
            EvidenceLinkRefs::default(),
        )
        .expect("attach parent evidence");
    store
        .update_task_status(
            &parent.task_id,
            TaskStatus::Open,
            "operator",
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Passed),
                ..TaskStatusUpdate::default()
            },
        )
        .expect("mark parent as verified");

    let child_a = store
        .create_subtask_with_options(
            &parent.task_id,
            "Step 1: Alpha",
            None,
            "operator",
            &TaskCreationOptions {
                verification_required: true,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create child a");
    let child_b = store
        .create_subtask_with_options(
            &parent.task_id,
            "Step 2: Beta",
            None,
            "operator",
            &TaskCreationOptions {
                verification_required: true,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create child b");

    for child in [&child_a, &child_b] {
        store
            .update_task_status(
                &child.task_id,
                TaskStatus::InProgress,
                "operator",
                TaskStatusUpdate::default(),
            )
            .expect("start child");
        store
            .add_evidence(
                &child.task_id,
                EvidenceSourceKind::ScriptVerification,
                "/tmp/verify-step.sh",
                "Verification script",
                Some("script verification passed\n\nResults: 1 passed, 0 failed"),
                EvidenceLinkRefs::default(),
            )
            .expect("attach child evidence");
        store
            .update_task_status(
                &child.task_id,
                TaskStatus::Completed,
                "operator",
                TaskStatusUpdate {
                    verification_state: Some(VerificationState::Passed),
                    closure_summary: Some("step complete"),
                    ..TaskStatusUpdate::default()
                },
            )
            .expect("complete child");
    }

    let refreshed_parent = store.get_task(&parent.task_id).expect("reload parent");
    assert_eq!(refreshed_parent.status, TaskStatus::Completed);
    assert_eq!(refreshed_parent.closed_by.as_deref(), Some("operator"));
}

#[test]
fn unverified_parent_stays_open_after_children_complete() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let parent = store
        .create_task_with_options(
            "Imported handoff",
            None,
            "operator",
            "/tmp/project",
            &TaskCreationOptions {
                verification_required: true,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create parent");
    let child = store
        .create_subtask_with_options(
            &parent.task_id,
            "Step 1: Alpha",
            None,
            "operator",
            &TaskCreationOptions {
                verification_required: true,
                ..TaskCreationOptions::default()
            },
        )
        .expect("create child");

    store
        .update_task_status(
            &child.task_id,
            TaskStatus::InProgress,
            "operator",
            TaskStatusUpdate::default(),
        )
        .expect("start child");
    store
        .add_evidence(
            &child.task_id,
            EvidenceSourceKind::ScriptVerification,
            "/tmp/verify-step.sh",
            "Verification script",
            Some("script verification passed\n\nResults: 1 passed, 0 failed"),
            EvidenceLinkRefs::default(),
        )
        .expect("attach child evidence");
    store
        .update_task_status(
            &child.task_id,
            TaskStatus::Completed,
            "operator",
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Passed),
                closure_summary: Some("step complete"),
                ..TaskStatusUpdate::default()
            },
        )
        .expect("complete child");

    let refreshed_parent = store.get_task(&parent.task_id).expect("reload parent");
    assert_eq!(refreshed_parent.status, TaskStatus::Open);
}

#[test]
fn task_deadline_updates_persist_and_record_history() {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("canopy.db");
    let store = Store::open(&db_path).expect("open store");

    let task = store
        .create_task(
            "Track deadline semantics",
            None,
            "operator",
            "/tmp/project",
            None,
        )
        .expect("create task");

    let with_execution_due = store
        .update_task_deadlines(
            &task.task_id,
            "operator",
            TaskDeadlineUpdate {
                due_at: Some("2026-03-30T18:00:00Z"),
                clear_due_at: false,
                review_due_at: None,
                clear_review_due_at: false,
                event_note: None,
            },
        )
        .expect("set execution due date");
    assert_eq!(
        with_execution_due.due_at.as_deref(),
        Some("2026-03-30T18:00:00Z")
    );
    assert_eq!(with_execution_due.review_due_at, None);

    store
        .update_task_status(
            &task.task_id,
            TaskStatus::ReviewRequired,
            "operator",
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Pending),
                ..TaskStatusUpdate::default()
            },
        )
        .expect("move task into review");
    assert!(
        store
            .update_task_deadlines(
                &task.task_id,
                "operator",
                TaskDeadlineUpdate {
                    due_at: Some("2026-04-01T00:00:00Z"),
                    clear_due_at: false,
                    review_due_at: None,
                    clear_review_due_at: false,
                    event_note: None,
                },
            )
            .is_err()
    );

    let with_review_due = store
        .update_task_deadlines(
            &task.task_id,
            "operator",
            TaskDeadlineUpdate {
                due_at: None,
                clear_due_at: false,
                review_due_at: Some("2026-03-31T12:00:00Z"),
                clear_review_due_at: false,
                event_note: None,
            },
        )
        .expect("set review due date");
    assert_eq!(
        with_review_due.review_due_at.as_deref(),
        Some("2026-03-31T12:00:00Z")
    );

    let cleared = store
        .update_task_deadlines(
            &task.task_id,
            "operator",
            TaskDeadlineUpdate {
                due_at: None,
                clear_due_at: true,
                review_due_at: None,
                clear_review_due_at: true,
                event_note: None,
            },
        )
        .expect("clear deadlines");
    assert!(cleared.due_at.is_none());
    assert!(cleared.review_due_at.is_none());

    let deadline_events = store
        .list_task_events(&task.task_id)
        .expect("list task events")
        .into_iter()
        .filter(|event| event.event_type == TaskEventType::DeadlineUpdated)
        .collect::<Vec<_>>();
    assert_eq!(deadline_events.len(), 3);
    assert!(deadline_events.iter().any(|event| {
        event
            .note
            .as_deref()
            .is_some_and(|note| note.contains("due_at"))
    }));
    assert!(deadline_events.iter().any(|event| {
        event
            .note
            .as_deref()
            .is_some_and(|note| note.contains("review_due_at"))
    }));
}
