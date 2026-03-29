use crate::models::{
    AgentAttention, AgentAttentionReason, AgentHeartbeatEvent, AgentHeartbeatSummary,
    AgentRegistration, ApiSnapshot, AttentionLevel, DeadlineState, ExecutionActionKind, Freshness,
    Handoff, HandoffAttention, HandoffAttentionReason, HandoffType, OperatorAction,
    OperatorActionKind, OperatorActionTargetKind, SnapshotAttentionSummary, SnapshotPreset, Task,
    TaskAssignment, TaskAttention, TaskAttentionReason, TaskDeadlineKind, TaskDeadlineSummary,
    TaskDetail, TaskEvent, TaskEventType, TaskExecutionSummary, TaskHeartbeatSummary,
    TaskOwnershipSummary, TaskPriority, TaskRelationship, TaskRelationshipKind,
    TaskRelationshipSummary, TaskSeverity, TaskSort, TaskStatus, TaskView, VerificationState,
    derive_review_cycle_context,
};
use crate::store::{Store, StoreError, StoreResult};
use std::collections::{HashMap, HashSet};
use time::format_description::well_known::Rfc3339;
use time::{
    OffsetDateTime, PrimitiveDateTime, format_description::FormatItem, macros::format_description,
};

const TASK_AGING_HOURS: i64 = 6;
const TASK_STALE_HOURS: i64 = 24;
const DEADLINE_SOON_HOURS: i64 = 24;
const HANDOFF_AGING_HOURS: i64 = 6;
const HANDOFF_STALE_HOURS: i64 = 24;
const HEARTBEAT_AGING_MINUTES: i64 = 15;
const HEARTBEAT_STALE_MINUTES: i64 = 60;
const SQLITE_TIMESTAMP_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

#[derive(Debug, Clone, Copy, Default)]
pub struct SnapshotOptions<'a> {
    pub project_root: Option<&'a str>,
    pub preset: Option<SnapshotPreset>,
    pub sort: Option<TaskSort>,
    pub view: Option<TaskView>,
    pub priority_at_least: Option<TaskPriority>,
    pub severity_at_least: Option<TaskSeverity>,
    pub acknowledged: Option<bool>,
    pub attention_at_least: Option<AttentionLevel>,
}

#[derive(Debug, Clone)]
struct ResolvedSnapshotOptions {
    project_root: Option<String>,
    sort: TaskSort,
    view: TaskView,
    priority_at_least: Option<TaskPriority>,
    severity_at_least: Option<TaskSeverity>,
    acknowledged: Option<bool>,
    attention_at_least: Option<AttentionLevel>,
}

/// Builds a stable read snapshot for operator surfaces.
///
/// # Errors
///
/// Returns an error if any underlying store query fails.
#[allow(clippy::too_many_lines)]
pub fn snapshot(store: &Store, options: SnapshotOptions<'_>) -> StoreResult<ApiSnapshot> {
    let options = resolve_snapshot_options(options);
    let mut agents = store.list_agents()?;
    if let Some(project_root) = options.project_root.as_deref() {
        agents.retain(|agent| agent.project_root == project_root);
    }

    let handoffs = store.list_handoffs(None)?;
    let mut tasks = store.list_tasks()?;
    let now = OffsetDateTime::now_utc();

    if let Some(project_root) = options.project_root.as_deref() {
        tasks.retain(|task| task.project_root == project_root);
    }

    let project_task_ids: HashSet<_> = tasks.iter().map(|task| task.task_id.clone()).collect();
    let all_task_events = store.list_all_task_events()?;
    let all_assignments = store.list_task_assignments(None)?;
    let project_task_events = all_task_events
        .iter()
        .filter(|event| project_task_ids.contains(&event.task_id))
        .cloned()
        .collect::<Vec<_>>();
    let project_assignments = all_assignments
        .iter()
        .filter(|assignment| project_task_ids.contains(&assignment.task_id))
        .cloned()
        .collect::<Vec<_>>();
    let relationships = store
        .list_task_relationships(None)?
        .into_iter()
        .filter(|relationship| {
            project_task_ids.contains(&relationship.source_task_id)
                && project_task_ids.contains(&relationship.target_task_id)
        })
        .collect::<Vec<_>>();
    let relationship_summaries = derive_task_relationship_summaries(&tasks, &relationships, now);
    let project_execution_summaries =
        derive_task_execution_summaries(&tasks, &project_task_events, now);
    let project_evidence = store
        .list_all_evidence()?
        .into_iter()
        .filter(|evidence| project_task_ids.contains(&evidence.task_id))
        .collect::<Vec<_>>();
    let accepted_handoff_follow_through_task_ids = derive_accepted_handoff_follow_through_task_ids(
        &tasks,
        &handoffs,
        &project_execution_summaries,
    );
    let due_soon_accepted_handoff_follow_through_task_ids =
        derive_accepted_handoff_follow_through_task_ids_with_freshness(
            &tasks,
            &handoffs,
            &project_execution_summaries,
            now,
            Freshness::Aging,
        );
    let overdue_accepted_handoff_follow_through_task_ids =
        derive_accepted_handoff_follow_through_task_ids_with_freshness(
            &tasks,
            &handoffs,
            &project_execution_summaries,
            now,
            Freshness::Stale,
        );
    let assigned_awaiting_claim_task_ids = derive_assigned_awaiting_claim_task_ids(
        &tasks,
        &project_assignments,
        &project_execution_summaries,
        &accepted_handoff_follow_through_task_ids,
    );
    let review_with_graph_pressure_task_ids =
        derive_review_with_graph_pressure_task_ids(&tasks, &relationship_summaries);
    let review_handoff_follow_through_task_ids =
        derive_review_handoff_follow_through_task_ids(&tasks, &handoffs, now);
    let review_decision_follow_through_task_ids =
        derive_review_decision_follow_through_task_ids(&tasks, &handoffs, now);
    let review_awaiting_support_task_ids =
        derive_review_awaiting_support_task_ids(&tasks, &project_task_events);
    let review_ready_for_decision_task_ids = derive_review_ready_for_decision_task_ids(
        &tasks,
        &project_task_events,
        &review_with_graph_pressure_task_ids,
        &review_handoff_follow_through_task_ids,
        &review_decision_follow_through_task_ids,
        &review_awaiting_support_task_ids,
    );
    let review_ready_for_closeout_task_ids = derive_review_ready_for_closeout_task_ids(
        &tasks,
        &project_task_events,
        &review_with_graph_pressure_task_ids,
        &review_handoff_follow_through_task_ids,
        &review_decision_follow_through_task_ids,
        &review_awaiting_support_task_ids,
    );
    let claimed_not_started_task_ids = derive_claimed_not_started_task_ids(
        &tasks,
        &project_execution_summaries,
        &accepted_handoff_follow_through_task_ids,
    );
    let paused_resumable_task_ids = derive_paused_resumable_task_ids(
        &tasks,
        &project_execution_summaries,
        &accepted_handoff_follow_through_task_ids,
    );
    let project_deadline_summaries = derive_task_deadline_summaries(&tasks, now);

    let all_heartbeats = store.list_all_agent_heartbeats()?;
    let agent_attention = derive_agent_attention(&agents, now);
    let handoff_attention = derive_handoff_attention(&handoffs, now);
    let task_attention = derive_task_attention(
        &tasks,
        &project_deadline_summaries,
        &handoffs,
        &agent_attention,
        &relationship_summaries,
        &assigned_awaiting_claim_task_ids,
        &review_with_graph_pressure_task_ids,
        &review_handoff_follow_through_task_ids,
        &review_decision_follow_through_task_ids,
        &review_awaiting_support_task_ids,
        &review_ready_for_decision_task_ids,
        &review_ready_for_closeout_task_ids,
        &claimed_not_started_task_ids,
        &paused_resumable_task_ids,
        &accepted_handoff_follow_through_task_ids,
        now,
    );
    let open_handoff_task_ids: HashSet<_> = handoffs
        .iter()
        .filter(|handoff| handoff.status == crate::models::HandoffStatus::Open)
        .map(|handoff| handoff.task_id.clone())
        .collect();
    let pending_handoff_acceptance_task_ids =
        derive_pending_handoff_acceptance_task_ids(&handoffs, now);
    let due_soon_handoff_acceptance_task_ids =
        derive_pending_handoff_acceptance_task_ids_with_freshness(&handoffs, now, Freshness::Aging);
    let overdue_handoff_acceptance_task_ids =
        derive_pending_handoff_acceptance_task_ids_with_freshness(&handoffs, now, Freshness::Stale);
    let task_attention_by_id: HashMap<_, _> = task_attention
        .iter()
        .map(|attention| (attention.task_id.clone(), attention.clone()))
        .collect();
    let deadline_summary_by_id: HashMap<_, _> = project_deadline_summaries
        .iter()
        .map(|summary| (summary.task_id.clone(), summary.clone()))
        .collect();
    let relationship_summary_by_id: HashMap<_, _> = relationship_summaries
        .iter()
        .map(|summary| (summary.task_id.clone(), summary.clone()))
        .collect();

    tasks.retain(|task| {
        matches_view(
            task,
            &open_handoff_task_ids,
            &pending_handoff_acceptance_task_ids,
            &due_soon_handoff_acceptance_task_ids,
            &overdue_handoff_acceptance_task_ids,
            &assigned_awaiting_claim_task_ids,
            &review_with_graph_pressure_task_ids,
            &review_handoff_follow_through_task_ids,
            &review_decision_follow_through_task_ids,
            &review_awaiting_support_task_ids,
            &review_ready_for_decision_task_ids,
            &review_ready_for_closeout_task_ids,
            &claimed_not_started_task_ids,
            &paused_resumable_task_ids,
            &accepted_handoff_follow_through_task_ids,
            &due_soon_accepted_handoff_follow_through_task_ids,
            &overdue_accepted_handoff_follow_through_task_ids,
            deadline_summary_by_id.get(&task.task_id),
            task_attention_by_id.get(&task.task_id),
            relationship_summary_by_id.get(&task.task_id),
            options.view,
        ) && matches_filters(task, task_attention_by_id.get(&task.task_id), &options)
    });
    sort_tasks(&mut tasks, options.sort, &task_attention_by_id);

    let task_ids: HashSet<_> = tasks.iter().map(|task| task.task_id.clone()).collect();
    let agent_ids: HashSet<_> = agents.iter().map(|agent| agent.agent_id.clone()).collect();
    let heartbeats = all_heartbeats
        .into_iter()
        .filter(|heartbeat| {
            agent_ids.contains(&heartbeat.agent_id)
                && heartbeat_matches_tasks(
                    heartbeat.current_task_id.as_deref(),
                    heartbeat.related_task_id.as_deref(),
                    &task_ids,
                )
        })
        .take(50)
        .collect::<Vec<_>>();
    let filtered_task_attention = task_attention
        .into_iter()
        .filter(|attention| task_ids.contains(&attention.task_id))
        .collect::<Vec<_>>();
    let filtered_deadline_summaries = project_deadline_summaries
        .into_iter()
        .filter(|summary| task_ids.contains(&summary.task_id))
        .collect::<Vec<_>>();
    let filtered_handoff_attention = handoff_attention
        .into_iter()
        .filter(|attention| task_ids.contains(&attention.task_id))
        .collect::<Vec<_>>();
    let filtered_agent_attention = agent_attention
        .into_iter()
        .filter(|attention| agent_ids.contains(&attention.agent_id))
        .collect::<Vec<_>>();
    let filtered_assignments = project_assignments
        .into_iter()
        .filter(|assignment| task_ids.contains(&assignment.task_id))
        .collect::<Vec<_>>();
    let ownership = derive_task_ownership_summaries(&tasks, &filtered_assignments);
    let task_heartbeat_summaries =
        derive_task_heartbeat_summaries(&tasks, &heartbeats, &filtered_agent_attention);
    let execution_summaries = project_execution_summaries
        .into_iter()
        .filter(|summary| task_ids.contains(&summary.task_id))
        .collect::<Vec<_>>();
    let agent_heartbeat_summaries =
        derive_agent_heartbeat_summaries(&agents, &heartbeats, &filtered_agent_attention);
    let filtered_handoffs = handoffs
        .into_iter()
        .filter(|handoff| task_ids.contains(&handoff.task_id))
        .collect::<Vec<_>>();
    let relationships = relationships
        .into_iter()
        .filter(|relationship| {
            task_ids.contains(&relationship.source_task_id)
                || task_ids.contains(&relationship.target_task_id)
        })
        .collect::<Vec<_>>();
    let filtered_relationship_summaries = relationship_summaries
        .into_iter()
        .filter(|summary| task_ids.contains(&summary.task_id))
        .collect::<Vec<_>>();
    let operator_actions = derive_operator_actions(
        &tasks,
        &filtered_task_attention,
        &filtered_deadline_summaries,
        &filtered_relationship_summaries,
        &execution_summaries,
        &filtered_handoffs,
        &filtered_handoff_attention,
    );
    let attention = summarize_attention(
        &filtered_task_attention,
        &filtered_handoff_attention,
        &filtered_agent_attention,
        &operator_actions,
    );

    Ok(ApiSnapshot {
        attention,
        agents,
        agent_attention: filtered_agent_attention,
        agent_heartbeat_summaries,
        heartbeats,
        tasks,
        task_attention: filtered_task_attention,
        deadline_summaries: filtered_deadline_summaries,
        task_heartbeat_summaries,
        execution_summaries,
        ownership,
        handoffs: filtered_handoffs,
        handoff_attention: filtered_handoff_attention,
        operator_actions,
        evidence: project_evidence
            .into_iter()
            .filter(|evidence| task_ids.contains(&evidence.task_id))
            .collect(),
        relationships,
        relationship_summaries: filtered_relationship_summaries,
    })
}

/// Builds a task-scoped read model without exposing raw tables directly.
///
/// # Errors
///
/// Returns an error if the task does not exist or any underlying store query
/// fails.
#[allow(clippy::too_many_lines)]
pub fn task_detail(store: &Store, task_id: &str) -> StoreResult<TaskDetail> {
    let task = store.get_task(task_id)?;
    let handoffs = store.list_handoffs(Some(task_id))?;
    let assignments = store.list_task_assignments(Some(task_id))?;
    let events = store.list_task_events(task_id)?;
    let messages = store.list_council_messages(task_id)?;
    let evidence = store.list_evidence(task_id)?;
    let heartbeats = store.list_task_heartbeats(task_id, 25)?;
    let agents = store.list_agents()?;
    let now = OffsetDateTime::now_utc();
    let execution_summary =
        derive_task_execution_summaries(std::slice::from_ref(&task), &events, now)
            .into_iter()
            .next()
            .ok_or(StoreError::Validation(
                "task execution summary could not be derived".to_string(),
            ))?;
    let accepted_handoff_follow_through_task_ids = derive_accepted_handoff_follow_through_task_ids(
        std::slice::from_ref(&task),
        &handoffs,
        std::slice::from_ref(&execution_summary),
    );
    let assigned_awaiting_claim_task_ids = derive_assigned_awaiting_claim_task_ids(
        std::slice::from_ref(&task),
        &assignments,
        std::slice::from_ref(&execution_summary),
        &accepted_handoff_follow_through_task_ids,
    );
    let claimed_not_started_task_ids = derive_claimed_not_started_task_ids(
        std::slice::from_ref(&task),
        std::slice::from_ref(&execution_summary),
        &accepted_handoff_follow_through_task_ids,
    );
    let paused_resumable_task_ids = derive_paused_resumable_task_ids(
        std::slice::from_ref(&task),
        std::slice::from_ref(&execution_summary),
        &accepted_handoff_follow_through_task_ids,
    );
    let related_handoff_agents: HashSet<_> = handoffs
        .iter()
        .flat_map(|handoff| [handoff.from_agent_id.as_str(), handoff.to_agent_id.as_str()])
        .collect();
    let agent_attention = derive_agent_attention(&agents, now)
        .into_iter()
        .filter(|attention| {
            attention.current_task_id.as_deref() == Some(task_id)
                || task.owner_agent_id.as_deref() == Some(attention.agent_id.as_str())
                || related_handoff_agents.contains(attention.agent_id.as_str())
                || heartbeats
                    .iter()
                    .any(|heartbeat| heartbeat.agent_id == attention.agent_id)
        })
        .collect::<Vec<_>>();
    let handoff_attention = derive_handoff_attention(&handoffs, now);
    let project_tasks = store
        .list_tasks()?
        .into_iter()
        .filter(|candidate| candidate.project_root == task.project_root)
        .collect::<Vec<_>>();
    let project_task_ids: HashSet<_> = project_tasks
        .iter()
        .map(|candidate| candidate.task_id.clone())
        .collect();
    let project_relationships = store
        .list_task_relationships(None)?
        .into_iter()
        .filter(|relationship| {
            project_task_ids.contains(&relationship.source_task_id)
                && project_task_ids.contains(&relationship.target_task_id)
        })
        .collect::<Vec<_>>();
    let relationship_summary =
        derive_task_relationship_summaries(&project_tasks, &project_relationships, now)
            .into_iter()
            .find(|summary| summary.task_id == task.task_id)
            .ok_or(StoreError::Validation(
                "task relationship summary could not be derived".to_string(),
            ))?;
    let review_with_graph_pressure_task_ids = derive_review_with_graph_pressure_task_ids(
        std::slice::from_ref(&task),
        std::slice::from_ref(&relationship_summary),
    );
    let review_handoff_follow_through_task_ids =
        derive_review_handoff_follow_through_task_ids(std::slice::from_ref(&task), &handoffs, now);
    let review_decision_follow_through_task_ids =
        derive_review_decision_follow_through_task_ids(std::slice::from_ref(&task), &handoffs, now);
    let review_awaiting_support_task_ids =
        derive_review_awaiting_support_task_ids(std::slice::from_ref(&task), &events);
    let review_ready_for_decision_task_ids = derive_review_ready_for_decision_task_ids(
        std::slice::from_ref(&task),
        &events,
        &review_with_graph_pressure_task_ids,
        &review_handoff_follow_through_task_ids,
        &review_decision_follow_through_task_ids,
        &review_awaiting_support_task_ids,
    );
    let review_ready_for_closeout_task_ids = derive_review_ready_for_closeout_task_ids(
        std::slice::from_ref(&task),
        &events,
        &review_with_graph_pressure_task_ids,
        &review_handoff_follow_through_task_ids,
        &review_decision_follow_through_task_ids,
        &review_awaiting_support_task_ids,
    );
    let deadline_summary = derive_task_deadline_summaries(std::slice::from_ref(&task), now)
        .into_iter()
        .next()
        .ok_or(StoreError::Validation(
            "task deadline summary could not be derived".to_string(),
        ))?;
    let attention = derive_task_attention(
        std::slice::from_ref(&task),
        std::slice::from_ref(&deadline_summary),
        &handoffs,
        &agent_attention,
        std::slice::from_ref(&relationship_summary),
        &assigned_awaiting_claim_task_ids,
        &review_with_graph_pressure_task_ids,
        &review_handoff_follow_through_task_ids,
        &review_decision_follow_through_task_ids,
        &review_awaiting_support_task_ids,
        &review_ready_for_decision_task_ids,
        &review_ready_for_closeout_task_ids,
        &claimed_not_started_task_ids,
        &paused_resumable_task_ids,
        &accepted_handoff_follow_through_task_ids,
        now,
    )
    .into_iter()
    .next()
    .ok_or(StoreError::Validation(
        "task attention could not be derived".to_string(),
    ))?;
    let ownership = derive_task_ownership_summaries(std::slice::from_ref(&task), &assignments)
        .into_iter()
        .next()
        .ok_or(StoreError::Validation(
            "task ownership could not be derived".to_string(),
        ))?;
    let heartbeat_summary =
        derive_task_heartbeat_summaries(std::slice::from_ref(&task), &heartbeats, &agent_attention)
            .into_iter()
            .next()
            .ok_or(StoreError::Validation(
                "task heartbeat summary could not be derived".to_string(),
            ))?;
    let related_agents = agents
        .into_iter()
        .filter(|agent| {
            agent_attention
                .iter()
                .any(|attention| attention.agent_id == agent.agent_id)
        })
        .collect::<Vec<_>>();
    let agent_heartbeat_summaries =
        derive_agent_heartbeat_summaries(&related_agents, &heartbeats, &agent_attention);
    let relationships = store.list_task_relationships(Some(task_id))?;
    let operator_actions = derive_operator_actions(
        std::slice::from_ref(&task),
        std::slice::from_ref(&attention),
        std::slice::from_ref(&deadline_summary),
        std::slice::from_ref(&relationship_summary),
        std::slice::from_ref(&execution_summary),
        &handoffs,
        &handoff_attention,
    );
    let allowed_actions = derive_allowed_actions(
        &task,
        &attention,
        &deadline_summary,
        &relationship_summary,
        &execution_summary,
        &handoffs,
        &handoff_attention,
        now,
    );

    Ok(TaskDetail {
        attention,
        agent_attention,
        agent_heartbeat_summaries,
        task,
        deadline_summary,
        ownership,
        heartbeat_summary,
        execution_summary,
        assignments,
        events,
        heartbeats,
        handoffs,
        handoff_attention,
        operator_actions,
        allowed_actions,
        messages,
        evidence,
        relationships,
        relationship_summary,
        related_tasks: store.list_related_tasks(task_id)?,
    })
}

fn resolve_snapshot_options(options: SnapshotOptions<'_>) -> ResolvedSnapshotOptions {
    let mut resolved = ResolvedSnapshotOptions {
        project_root: options.project_root.map(str::to_owned),
        sort: TaskSort::Status,
        view: TaskView::All,
        priority_at_least: None,
        severity_at_least: None,
        acknowledged: None,
        attention_at_least: None,
    };

    if let Some(preset) = options.preset {
        apply_preset(&mut resolved, preset);
    }

    if let Some(view) = options.view {
        resolved.view = view;
    }
    if let Some(sort) = options.sort {
        resolved.sort = sort;
    }
    if let Some(priority) = options.priority_at_least {
        resolved.priority_at_least = Some(priority);
    }
    if let Some(severity) = options.severity_at_least {
        resolved.severity_at_least = Some(severity);
    }
    if let Some(acknowledged) = options.acknowledged {
        resolved.acknowledged = Some(acknowledged);
    }
    if let Some(level) = options.attention_at_least {
        resolved.attention_at_least = Some(level);
    }

    resolved
}

#[allow(clippy::too_many_lines)]
fn apply_preset(options: &mut ResolvedSnapshotOptions, preset: SnapshotPreset) {
    match preset {
        SnapshotPreset::Default => {}
        SnapshotPreset::Attention => {
            options.view = TaskView::Attention;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::ReviewQueue => {
            options.view = TaskView::Review;
            options.sort = TaskSort::Verification;
            options.acknowledged = Some(false);
        }
        SnapshotPreset::ReviewWithGraphPressure => {
            options.view = TaskView::ReviewWithGraphPressure;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::ReviewHandoffFollowThrough => {
            options.view = TaskView::ReviewHandoffFollowThrough;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::ReviewDecisionFollowThrough => {
            options.view = TaskView::ReviewDecisionFollowThrough;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::ReviewAwaitingSupport => {
            options.view = TaskView::ReviewAwaitingSupport;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::ReviewReadyForDecision => {
            options.view = TaskView::ReviewReadyForDecision;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::ReviewReadyForCloseout => {
            options.view = TaskView::ReviewReadyForCloseout;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::Unclaimed => {
            options.view = TaskView::Unclaimed;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::AssignedAwaitingClaim => {
            options.view = TaskView::AssignedAwaitingClaim;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::ClaimedNotStarted => {
            options.view = TaskView::ClaimedNotStarted;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::InProgress => {
            options.view = TaskView::InProgress;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::Stalled => {
            options.view = TaskView::Stalled;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::PausedResumable => {
            options.view = TaskView::PausedResumable;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::DueSoon => {
            options.view = TaskView::DueSoon;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::DueSoonExecution => {
            options.view = TaskView::DueSoonExecution;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::DueSoonReview => {
            options.view = TaskView::DueSoonReview;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::OverdueExecution => {
            options.view = TaskView::OverdueExecution;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::OverdueExecutionOwned => {
            options.view = TaskView::OverdueExecutionOwned;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::OverdueExecutionUnclaimed => {
            options.view = TaskView::OverdueExecutionUnclaimed;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::OverdueReview => {
            options.view = TaskView::OverdueReview;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::AwaitingHandoffAcceptance => {
            options.view = TaskView::AwaitingHandoffAcceptance;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::DueSoonHandoffAcceptance => {
            options.view = TaskView::DueSoonHandoffAcceptance;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::OverdueHandoffAcceptance => {
            options.view = TaskView::OverdueHandoffAcceptance;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::AcceptedHandoffFollowThrough => {
            options.view = TaskView::AcceptedHandoffFollowThrough;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::DueSoonAcceptedHandoffFollowThrough => {
            options.view = TaskView::DueSoonAcceptedHandoffFollowThrough;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::OverdueAcceptedHandoffFollowThrough => {
            options.view = TaskView::OverdueAcceptedHandoffFollowThrough;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::Blocked => {
            options.view = TaskView::Blocked;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::BlockedByDependencies => {
            options.view = TaskView::BlockedByDependencies;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::Handoffs => {
            options.view = TaskView::Handoffs;
            options.sort = TaskSort::UpdatedAt;
        }
        SnapshotPreset::FollowUpChains => {
            options.view = TaskView::FollowUpChains;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::Critical => {
            options.view = TaskView::Attention;
            options.sort = TaskSort::Attention;
            options.attention_at_least = Some(AttentionLevel::Critical);
        }
        SnapshotPreset::Unacknowledged => {
            options.view = TaskView::Attention;
            options.sort = TaskSort::Attention;
            options.acknowledged = Some(false);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn matches_view(
    task: &Task,
    open_handoff_task_ids: &HashSet<String>,
    pending_handoff_acceptance_task_ids: &HashSet<String>,
    due_soon_handoff_acceptance_task_ids: &HashSet<String>,
    overdue_handoff_acceptance_task_ids: &HashSet<String>,
    assigned_awaiting_claim_task_ids: &HashSet<String>,
    review_with_graph_pressure_task_ids: &HashSet<String>,
    review_handoff_follow_through_task_ids: &HashSet<String>,
    review_decision_follow_through_task_ids: &HashSet<String>,
    review_awaiting_support_task_ids: &HashSet<String>,
    review_ready_for_decision_task_ids: &HashSet<String>,
    review_ready_for_closeout_task_ids: &HashSet<String>,
    claimed_not_started_task_ids: &HashSet<String>,
    paused_resumable_task_ids: &HashSet<String>,
    accepted_handoff_follow_through_task_ids: &HashSet<String>,
    due_soon_accepted_handoff_follow_through_task_ids: &HashSet<String>,
    overdue_accepted_handoff_follow_through_task_ids: &HashSet<String>,
    deadline_summary: Option<&TaskDeadlineSummary>,
    task_attention: Option<&TaskAttention>,
    relationship_summary: Option<&TaskRelationshipSummary>,
    view: TaskView,
) -> bool {
    match view {
        TaskView::All => true,
        TaskView::Active => matches!(
            task.status,
            TaskStatus::Open | TaskStatus::Assigned | TaskStatus::InProgress
        ),
        TaskView::Unclaimed => task.status == TaskStatus::Open && task.owner_agent_id.is_none(),
        TaskView::AssignedAwaitingClaim => assigned_awaiting_claim_task_ids.contains(&task.task_id),
        TaskView::ClaimedNotStarted => claimed_not_started_task_ids.contains(&task.task_id),
        TaskView::InProgress => task.status == TaskStatus::InProgress,
        TaskView::Stalled => {
            matches!(task.status, TaskStatus::Assigned | TaskStatus::InProgress)
                && task_attention.is_some_and(|attention| {
                    matches!(
                        attention.owner_heartbeat_freshness,
                        Some(Freshness::Stale | Freshness::Missing)
                    )
                })
        }
        TaskView::PausedResumable => paused_resumable_task_ids.contains(&task.task_id),
        TaskView::DueSoon => deadline_summary
            .is_some_and(|summary| summary.active_deadline_state == DeadlineState::DueSoon),
        TaskView::DueSoonExecution => deadline_summary
            .is_some_and(|summary| summary.execution_state == DeadlineState::DueSoon),
        TaskView::DueSoonReview => {
            deadline_summary.is_some_and(|summary| summary.review_state == DeadlineState::DueSoon)
        }
        TaskView::OverdueExecution => deadline_summary
            .is_some_and(|summary| summary.execution_state == DeadlineState::Overdue),
        TaskView::OverdueExecutionOwned => {
            task.owner_agent_id.is_some()
                && deadline_summary
                    .is_some_and(|summary| summary.execution_state == DeadlineState::Overdue)
        }
        TaskView::OverdueExecutionUnclaimed => {
            task.owner_agent_id.is_none()
                && deadline_summary
                    .is_some_and(|summary| summary.execution_state == DeadlineState::Overdue)
        }
        TaskView::OverdueReview => {
            deadline_summary.is_some_and(|summary| summary.review_state == DeadlineState::Overdue)
        }
        TaskView::AwaitingHandoffAcceptance => {
            pending_handoff_acceptance_task_ids.contains(&task.task_id)
        }
        TaskView::DueSoonHandoffAcceptance => {
            due_soon_handoff_acceptance_task_ids.contains(&task.task_id)
        }
        TaskView::OverdueHandoffAcceptance => {
            overdue_handoff_acceptance_task_ids.contains(&task.task_id)
        }
        TaskView::AcceptedHandoffFollowThrough => {
            accepted_handoff_follow_through_task_ids.contains(&task.task_id)
        }
        TaskView::DueSoonAcceptedHandoffFollowThrough => {
            due_soon_accepted_handoff_follow_through_task_ids.contains(&task.task_id)
        }
        TaskView::OverdueAcceptedHandoffFollowThrough => {
            overdue_accepted_handoff_follow_through_task_ids.contains(&task.task_id)
        }
        TaskView::Handoffs => open_handoff_task_ids.contains(&task.task_id),
        TaskView::Blocked => {
            task.status == TaskStatus::Blocked
                || task.verification_state == VerificationState::Failed
        }
        TaskView::BlockedByDependencies => {
            task.status == TaskStatus::Blocked
                && relationship_summary.is_some_and(|summary| summary.blocker_count > 0)
        }
        TaskView::ReviewWithGraphPressure => {
            review_with_graph_pressure_task_ids.contains(&task.task_id)
        }
        TaskView::ReviewHandoffFollowThrough => {
            review_handoff_follow_through_task_ids.contains(&task.task_id)
        }
        TaskView::ReviewDecisionFollowThrough => {
            review_decision_follow_through_task_ids.contains(&task.task_id)
        }
        TaskView::ReviewAwaitingSupport => review_awaiting_support_task_ids.contains(&task.task_id),
        TaskView::ReviewReadyForDecision => {
            review_ready_for_decision_task_ids.contains(&task.task_id)
        }
        TaskView::ReviewReadyForCloseout => {
            review_ready_for_closeout_task_ids.contains(&task.task_id)
        }
        TaskView::Review => {
            task.status == TaskStatus::ReviewRequired
                || task.verification_state == VerificationState::Pending
        }
        TaskView::FollowUpChains => relationship_summary.is_some_and(|summary| {
            summary.follow_up_parent_count > 0 || summary.follow_up_child_count > 0
        }),
        TaskView::Attention => {
            task_attention.is_some_and(|attention| attention.level != AttentionLevel::Normal)
        }
    }
}

fn matches_filters(
    task: &Task,
    task_attention: Option<&TaskAttention>,
    options: &ResolvedSnapshotOptions,
) -> bool {
    let priority_ok = options
        .priority_at_least
        .is_none_or(|minimum| task_priority_rank(task.priority) >= task_priority_rank(minimum));
    let severity_ok = options
        .severity_at_least
        .is_none_or(|minimum| task_severity_rank(task.severity) >= task_severity_rank(minimum));
    let acknowledged_ok = options
        .acknowledged
        .is_none_or(|acknowledged| acknowledged == task.acknowledged_at.is_some());
    let attention_ok = options.attention_at_least.is_none_or(|minimum| {
        task_attention.is_some_and(|attention| {
            attention_level_rank(attention.level) >= attention_level_rank(minimum)
        })
    });

    priority_ok && severity_ok && acknowledged_ok && attention_ok
}

fn sort_tasks(tasks: &mut [Task], sort: TaskSort, task_attention: &HashMap<String, TaskAttention>) {
    tasks.sort_by(|left, right| match sort {
        TaskSort::Title => left.title.cmp(&right.title),
        TaskSort::UpdatedAt => compare_timestamp_desc(&left.updated_at, &right.updated_at)
            .then_with(|| left.title.cmp(&right.title)),
        TaskSort::CreatedAt => compare_timestamp_desc(&left.created_at, &right.created_at)
            .then_with(|| left.title.cmp(&right.title)),
        TaskSort::Verification => verification_rank(left.verification_state)
            .cmp(&verification_rank(right.verification_state))
            .then_with(|| left.title.cmp(&right.title)),
        TaskSort::Priority => task_priority_rank(right.priority)
            .cmp(&task_priority_rank(left.priority))
            .then_with(|| left.title.cmp(&right.title)),
        TaskSort::Severity => task_severity_rank(right.severity)
            .cmp(&task_severity_rank(left.severity))
            .then_with(|| left.title.cmp(&right.title)),
        TaskSort::Attention => attention_sort_key(task_attention.get(&left.task_id))
            .cmp(&attention_sort_key(task_attention.get(&right.task_id)))
            .then_with(|| left.title.cmp(&right.title)),
        TaskSort::Status => status_rank(left.status)
            .cmp(&status_rank(right.status))
            .then_with(|| left.title.cmp(&right.title)),
    });
}

fn is_open_task_status(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Open
            | TaskStatus::Assigned
            | TaskStatus::InProgress
            | TaskStatus::Blocked
            | TaskStatus::ReviewRequired
    )
}

fn derive_task_relationship_summaries(
    tasks: &[Task],
    relationships: &[TaskRelationship],
    now: OffsetDateTime,
) -> Vec<TaskRelationshipSummary> {
    let tasks_by_id: HashMap<_, _> = tasks
        .iter()
        .map(|task| (task.task_id.as_str(), task))
        .collect();
    let mut summaries: HashMap<String, TaskRelationshipSummary> = tasks
        .iter()
        .map(|task| {
            (
                task.task_id.clone(),
                TaskRelationshipSummary {
                    task_id: task.task_id.clone(),
                    blocker_count: 0,
                    active_blocker_count: 0,
                    stale_blocker_count: 0,
                    blocking_count: 0,
                    follow_up_parent_count: 0,
                    follow_up_child_count: 0,
                    open_follow_up_child_count: 0,
                },
            )
        })
        .collect();

    for relationship in relationships {
        let Some(source_task) = tasks_by_id.get(relationship.source_task_id.as_str()) else {
            continue;
        };
        let Some(target_task) = tasks_by_id.get(relationship.target_task_id.as_str()) else {
            continue;
        };

        match relationship.kind {
            TaskRelationshipKind::FollowUp => {
                if let Some(summary) = summaries.get_mut(&relationship.source_task_id) {
                    summary.follow_up_child_count += 1;
                    if is_open_task_status(target_task.status) {
                        summary.open_follow_up_child_count += 1;
                    }
                }
                if let Some(summary) = summaries.get_mut(&relationship.target_task_id) {
                    summary.follow_up_parent_count += 1;
                }
            }
            TaskRelationshipKind::Blocks => {
                if let Some(summary) = summaries.get_mut(&relationship.source_task_id) {
                    summary.blocking_count += 1;
                }
                if let Some(summary) = summaries.get_mut(&relationship.target_task_id) {
                    summary.blocker_count += 1;
                    if relationship_blocker_is_stale(source_task, now) {
                        summary.stale_blocker_count += 1;
                    } else {
                        summary.active_blocker_count += 1;
                    }
                }
            }
        }
    }

    tasks
        .iter()
        .map(|task| {
            summaries
                .remove(&task.task_id)
                .unwrap_or(TaskRelationshipSummary {
                    task_id: task.task_id.clone(),
                    blocker_count: 0,
                    active_blocker_count: 0,
                    stale_blocker_count: 0,
                    blocking_count: 0,
                    follow_up_parent_count: 0,
                    follow_up_child_count: 0,
                    open_follow_up_child_count: 0,
                })
        })
        .collect()
}

fn relationship_blocker_is_stale(task: &Task, now: OffsetDateTime) -> bool {
    if !is_open_task_status(task.status) {
        return true;
    }

    timestamp_freshness(&task.updated_at, now, TASK_AGING_HOURS, TASK_STALE_HOURS)
        == Freshness::Stale
}

fn status_rank(status: TaskStatus) -> u8 {
    match status {
        TaskStatus::InProgress => 0,
        TaskStatus::ReviewRequired => 1,
        TaskStatus::Blocked => 2,
        TaskStatus::Assigned => 3,
        TaskStatus::Open => 4,
        TaskStatus::Completed => 5,
        TaskStatus::Closed => 6,
        TaskStatus::Cancelled => 7,
    }
}

fn verification_rank(state: VerificationState) -> u8 {
    match state {
        VerificationState::Failed => 0,
        VerificationState::Pending => 1,
        VerificationState::Unknown => 2,
        VerificationState::Passed => 3,
    }
}

fn task_priority_rank(priority: TaskPriority) -> u8 {
    match priority {
        TaskPriority::Low => 0,
        TaskPriority::Medium => 1,
        TaskPriority::High => 2,
        TaskPriority::Critical => 3,
    }
}

fn task_severity_rank(severity: TaskSeverity) -> u8 {
    match severity {
        TaskSeverity::None => 0,
        TaskSeverity::Low => 1,
        TaskSeverity::Medium => 2,
        TaskSeverity::High => 3,
        TaskSeverity::Critical => 4,
    }
}

fn attention_level_rank(level: AttentionLevel) -> u8 {
    match level {
        AttentionLevel::Normal => 0,
        AttentionLevel::NeedsAttention => 1,
        AttentionLevel::Critical => 2,
    }
}

fn task_level(level: AttentionLevel) -> AttentionLevel {
    if level == AttentionLevel::Normal {
        AttentionLevel::NeedsAttention
    } else {
        level
    }
}

fn attention_sort_key(attention: Option<&TaskAttention>) -> (u8, u8, u8, u8) {
    let Some(attention) = attention else {
        return (2, 4, 1, 1);
    };
    (
        2 - attention_level_rank(attention.level),
        freshness_sort_rank(attention.freshness),
        u8::from(attention.acknowledged),
        u8::try_from(attention.reasons.len()).unwrap_or(u8::MAX),
    )
}

fn derive_task_ownership_summaries(
    tasks: &[Task],
    assignments: &[TaskAssignment],
) -> Vec<TaskOwnershipSummary> {
    let mut assignments_by_task: HashMap<&str, Vec<&TaskAssignment>> = HashMap::new();
    for assignment in assignments {
        assignments_by_task
            .entry(assignment.task_id.as_str())
            .or_default()
            .push(assignment);
    }

    tasks
        .iter()
        .map(|task| {
            let history = assignments_by_task
                .get(task.task_id.as_str())
                .map_or(&[][..], Vec::as_slice);
            let last_assignment = history.last().copied();
            let reassignment_count = history
                .windows(2)
                .filter(|window| window[0].assigned_to != window[1].assigned_to)
                .count();

            TaskOwnershipSummary {
                task_id: task.task_id.clone(),
                current_owner_agent_id: task.owner_agent_id.clone(),
                assignment_count: history.len(),
                reassignment_count,
                last_assigned_to: last_assignment.map(|assignment| assignment.assigned_to.clone()),
                last_assigned_by: last_assignment.map(|assignment| assignment.assigned_by.clone()),
                last_assigned_at: last_assignment.map(|assignment| assignment.assigned_at.clone()),
                last_assignment_reason: last_assignment
                    .and_then(|assignment| assignment.reason.clone()),
            }
        })
        .collect()
}

fn derive_task_heartbeat_summaries(
    tasks: &[Task],
    heartbeats: &[AgentHeartbeatEvent],
    agent_attention: &[AgentAttention],
) -> Vec<TaskHeartbeatSummary> {
    let attention_by_agent: HashMap<_, _> = agent_attention
        .iter()
        .map(|attention| (attention.agent_id.as_str(), attention))
        .collect();

    tasks
        .iter()
        .map(|task| {
            let related_heartbeats = heartbeats
                .iter()
                .filter(|heartbeat| {
                    heartbeat.current_task_id.as_deref() == Some(task.task_id.as_str())
                        || heartbeat.related_task_id.as_deref() == Some(task.task_id.as_str())
                })
                .collect::<Vec<_>>();
            let mut related_agents: HashSet<&str> = related_heartbeats
                .iter()
                .map(|heartbeat| heartbeat.agent_id.as_str())
                .collect();
            if let Some(owner_agent_id) = task.owner_agent_id.as_deref() {
                related_agents.insert(owner_agent_id);
            }

            let mut fresh_agents = 0;
            let mut aging_agents = 0;
            let mut stale_agents = 0;
            let mut missing_agents = 0;

            for agent_id in &related_agents {
                match attention_by_agent
                    .get(agent_id)
                    .map_or(Freshness::Missing, |attention| attention.freshness)
                {
                    Freshness::Fresh => fresh_agents += 1,
                    Freshness::Aging => aging_agents += 1,
                    Freshness::Stale => stale_agents += 1,
                    Freshness::Missing => missing_agents += 1,
                }
            }

            TaskHeartbeatSummary {
                task_id: task.task_id.clone(),
                heartbeat_count: related_heartbeats.len(),
                related_agent_count: related_agents.len(),
                fresh_agents,
                aging_agents,
                stale_agents,
                missing_agents,
                last_heartbeat_at: latest_heartbeat_timestamp(&related_heartbeats),
            }
        })
        .collect()
}

fn derive_task_execution_summaries(
    tasks: &[Task],
    events: &[TaskEvent],
    now: OffsetDateTime,
) -> Vec<TaskExecutionSummary> {
    let mut events_by_task: HashMap<&str, Vec<&TaskEvent>> = HashMap::new();
    for event in events {
        events_by_task
            .entry(event.task_id.as_str())
            .or_default()
            .push(event);
    }

    tasks
        .iter()
        .map(|task| {
            let history = events_by_task
                .get(task.task_id.as_str())
                .map_or(&[][..], Vec::as_slice);
            let mut claim_count = 0;
            let mut run_count = 0;
            let mut pause_count = 0;
            let mut yield_count = 0;
            let mut completion_count = 0;
            let mut claimed_at = None;
            let mut started_at = None;
            let mut last_execution_at = None;
            let mut last_execution_action = None;
            let mut last_execution_agent_id = None;
            let mut total_execution_seconds = 0_i64;
            let mut active_start_at = None;

            for event in history
                .iter()
                .filter(|event| event.event_type == TaskEventType::ExecutionUpdated)
            {
                let Some(action) = event.execution_action else {
                    continue;
                };
                last_execution_at = Some(event.created_at.clone());
                last_execution_action = Some(action);
                last_execution_agent_id = Some(event.actor.clone());
                match action {
                    ExecutionActionKind::ClaimTask => {
                        claim_count += 1;
                        claimed_at.get_or_insert_with(|| event.created_at.clone());
                    }
                    ExecutionActionKind::StartTask | ExecutionActionKind::ResumeTask => {
                        run_count += 1;
                        started_at = Some(event.created_at.clone());
                        active_start_at = Some(event.created_at.clone());
                    }
                    ExecutionActionKind::PauseTask => {
                        pause_count += 1;
                        total_execution_seconds += event.execution_duration_seconds.unwrap_or(0);
                        active_start_at = None;
                    }
                    ExecutionActionKind::YieldTask => {
                        yield_count += 1;
                        total_execution_seconds += event.execution_duration_seconds.unwrap_or(0);
                        active_start_at = None;
                    }
                    ExecutionActionKind::CompleteTask => {
                        completion_count += 1;
                        total_execution_seconds += event.execution_duration_seconds.unwrap_or(0);
                        active_start_at = None;
                    }
                }
            }

            let active_execution_seconds = if task.status == TaskStatus::InProgress {
                active_start_at.as_deref().and_then(|raw| {
                    parse_timestamp(raw).map(|started_at| (now - started_at).whole_seconds().max(0))
                })
            } else {
                None
            };

            TaskExecutionSummary {
                task_id: task.task_id.clone(),
                claim_count,
                run_count,
                pause_count,
                yield_count,
                completion_count,
                claimed_at,
                started_at,
                last_execution_at,
                last_execution_action,
                last_execution_agent_id,
                total_execution_seconds,
                active_execution_seconds,
            }
        })
        .collect()
}

fn derive_agent_heartbeat_summaries(
    agents: &[AgentRegistration],
    heartbeats: &[AgentHeartbeatEvent],
    agent_attention: &[AgentAttention],
) -> Vec<AgentHeartbeatSummary> {
    let attention_by_agent: HashMap<_, _> = agent_attention
        .iter()
        .map(|attention| (attention.agent_id.as_str(), attention))
        .collect();

    agents
        .iter()
        .map(|agent| {
            let history = heartbeats
                .iter()
                .filter(|heartbeat| heartbeat.agent_id == agent.agent_id)
                .collect::<Vec<_>>();
            let latest = latest_heartbeat_event(&history);
            AgentHeartbeatSummary {
                agent_id: agent.agent_id.clone(),
                current_task_id: agent.current_task_id.clone(),
                heartbeat_count: history.len(),
                last_heartbeat_at: latest.map(|heartbeat| heartbeat.created_at.clone()),
                last_status: latest.map(|heartbeat| heartbeat.status),
                freshness: attention_by_agent
                    .get(agent.agent_id.as_str())
                    .map_or(Freshness::Missing, |attention| attention.freshness),
            }
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
fn derive_operator_actions(
    tasks: &[Task],
    task_attention: &[TaskAttention],
    deadline_summaries: &[TaskDeadlineSummary],
    relationship_summaries: &[TaskRelationshipSummary],
    execution_summaries: &[TaskExecutionSummary],
    handoffs: &[Handoff],
    handoff_attention: &[HandoffAttention],
) -> Vec<OperatorAction> {
    let task_attention_by_id: HashMap<_, _> = task_attention
        .iter()
        .map(|attention| (attention.task_id.as_str(), attention))
        .collect();
    let deadline_summary_by_task_id: HashMap<_, _> = deadline_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();
    let relationship_summary_by_task_id: HashMap<_, _> = relationship_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();
    let execution_summary_by_task_id: HashMap<_, _> = execution_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();
    let handoff_attention_by_id: HashMap<_, _> = handoff_attention
        .iter()
        .map(|attention| (attention.handoff_id.as_str(), attention))
        .collect();

    let mut actions = Vec::new();

    for task in tasks {
        let Some(attention) = task_attention_by_id.get(task.task_id.as_str()) else {
            continue;
        };
        let deadline_summary = deadline_summary_by_task_id
            .get(task.task_id.as_str())
            .copied();
        let relationship_summary = relationship_summary_by_task_id
            .get(task.task_id.as_str())
            .copied();
        let execution_summary = execution_summary_by_task_id
            .get(task.task_id.as_str())
            .copied();

        if attention
            .reasons
            .contains(&TaskAttentionReason::Unacknowledged)
        {
            actions.push(OperatorAction {
                action_id: format!("task:{}:acknowledge", task.task_id),
                kind: OperatorActionKind::AcknowledgeTask,
                target_kind: OperatorActionTargetKind::Task,
                level: attention.level,
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Acknowledge {}", task.title),
                summary: "Task attention has not been acknowledged yet.".to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if task.status == TaskStatus::ReviewRequired
            || matches!(
                task.verification_state,
                VerificationState::Pending | VerificationState::Failed
            )
        {
            actions.push(OperatorAction {
                action_id: format!("task:{}:verify", task.task_id),
                kind: OperatorActionKind::VerifyTask,
                target_kind: OperatorActionTargetKind::Task,
                level: if attention.level == AttentionLevel::Normal {
                    AttentionLevel::NeedsAttention
                } else {
                    attention.level
                },
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Review {}", task.title),
                summary: "Task is waiting on verification or operator review.".to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if let Some(deadline_summary) = deadline_summary
            && matches!(
                deadline_summary.active_deadline_state,
                DeadlineState::DueSoon | DeadlineState::Overdue
            )
            && let Some(deadline_kind) = deadline_summary.active_deadline_kind
        {
            let (kind, label, summary) =
                match (deadline_kind, deadline_summary.active_deadline_state) {
                    (TaskDeadlineKind::Execution, DeadlineState::DueSoon) => (
                        OperatorActionKind::SetTaskDueAt,
                        format!("Adjust execution due date for {}", task.title),
                        "Execution deadline is approaching and may need adjustment.".to_string(),
                    ),
                    (TaskDeadlineKind::Execution, DeadlineState::Overdue) => (
                        OperatorActionKind::SetTaskDueAt,
                        format!("Resolve overdue execution deadline for {}", task.title),
                        "Execution deadline has passed and needs operator follow-through."
                            .to_string(),
                    ),
                    (TaskDeadlineKind::Review, DeadlineState::DueSoon) => (
                        OperatorActionKind::SetReviewDueAt,
                        format!("Adjust review due date for {}", task.title),
                        "Review deadline is approaching and may need adjustment.".to_string(),
                    ),
                    (TaskDeadlineKind::Review, DeadlineState::Overdue) => (
                        OperatorActionKind::SetReviewDueAt,
                        format!("Resolve overdue review deadline for {}", task.title),
                        "Review deadline has passed and needs operator follow-through.".to_string(),
                    ),
                    (_, DeadlineState::None | DeadlineState::Scheduled) => unreachable!(),
                };
            actions.push(OperatorAction {
                action_id: format!("task:{}:deadline:{kind}", task.task_id),
                kind,
                target_kind: OperatorActionTargetKind::Task,
                level: task_level(attention.level),
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: label,
                summary,
                due_at: deadline_summary.active_deadline_at.clone(),
                expires_at: None,
            });
        }

        if attention
            .reasons
            .contains(&TaskAttentionReason::ReviewReadyForDecision)
        {
            actions.push(OperatorAction {
                action_id: format!("task:{}:record_decision", task.task_id),
                kind: OperatorActionKind::RecordDecision,
                target_kind: OperatorActionTargetKind::Task,
                level: if attention.level == AttentionLevel::Normal {
                    AttentionLevel::NeedsAttention
                } else {
                    attention.level
                },
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Record decision for {}", task.title),
                summary: "Persist the review decision for the current cycle.".to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if attention
            .reasons
            .contains(&TaskAttentionReason::ReviewReadyForCloseout)
        {
            actions.push(OperatorAction {
                action_id: format!("task:{}:close", task.task_id),
                kind: OperatorActionKind::CloseTask,
                target_kind: OperatorActionTargetKind::Task,
                level: if attention.level == AttentionLevel::Normal {
                    AttentionLevel::NeedsAttention
                } else {
                    attention.level
                },
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Close {}", task.title),
                summary: "Finalize review closeout and mark the task complete.".to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if task.owner_agent_id.is_some()
            && attention.reasons.iter().any(|reason| {
                matches!(
                    reason,
                    TaskAttentionReason::Blocked
                        | TaskAttentionReason::StaleOwnerHeartbeat
                        | TaskAttentionReason::MissingOwnerHeartbeat
                )
            })
        {
            actions.push(OperatorAction {
                action_id: format!("task:{}:reassign", task.task_id),
                kind: OperatorActionKind::ReassignTask,
                target_kind: OperatorActionTargetKind::Task,
                level: attention.level,
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Reassign {}", task.title),
                summary: "Owner state suggests the task may need reassignment or escalation."
                    .to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if task.status == TaskStatus::Open
            && task.owner_agent_id.is_none()
            && relationship_summary.is_none_or(|summary| summary.active_blocker_count == 0)
        {
            actions.push(OperatorAction {
                action_id: format!("task:{}:claim", task.task_id),
                kind: OperatorActionKind::ClaimTask,
                target_kind: OperatorActionTargetKind::Task,
                level: task_level(attention.level),
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: None,
                title: format!("Claim {}", task.title),
                summary: "Claim this unowned task for agent execution.".to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if task.status == TaskStatus::Assigned && task.owner_agent_id.is_some() {
            let has_prior_execution =
                execution_summary.is_some_and(|summary| summary.run_count > 0);
            actions.push(OperatorAction {
                action_id: format!(
                    "task:{}:{}",
                    task.task_id,
                    if has_prior_execution {
                        "resume"
                    } else {
                        "start"
                    }
                ),
                kind: if has_prior_execution {
                    OperatorActionKind::ResumeTask
                } else {
                    OperatorActionKind::StartTask
                },
                target_kind: OperatorActionTargetKind::Task,
                level: task_level(attention.level),
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: if has_prior_execution {
                    format!("Resume {}", task.title)
                } else {
                    format!("Start {}", task.title)
                },
                summary: if has_prior_execution {
                    "Resume execution on this previously started task.".to_string()
                } else {
                    "Move this claimed task into active execution.".to_string()
                },
                due_at: None,
                expires_at: None,
            });
        }

        if matches!(task.status, TaskStatus::Assigned | TaskStatus::InProgress)
            && task.owner_agent_id.is_some()
        {
            actions.push(OperatorAction {
                action_id: format!("task:{}:yield", task.task_id),
                kind: OperatorActionKind::YieldTask,
                target_kind: OperatorActionTargetKind::Task,
                level: task_level(attention.level),
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Yield {}", task.title),
                summary: "Release this task back to the unclaimed queue.".to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if task.status == TaskStatus::InProgress && task.owner_agent_id.is_some() {
            actions.push(OperatorAction {
                action_id: format!("task:{}:pause", task.task_id),
                kind: OperatorActionKind::PauseTask,
                target_kind: OperatorActionTargetKind::Task,
                level: task_level(attention.level),
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Pause {}", task.title),
                summary: "Pause active execution without yielding ownership.".to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if matches!(task.status, TaskStatus::Assigned | TaskStatus::InProgress)
            && task.owner_agent_id.is_some()
            && task.status != TaskStatus::Blocked
        {
            actions.push(OperatorAction {
                action_id: format!("task:{}:complete", task.task_id),
                kind: OperatorActionKind::CompleteTask,
                target_kind: OperatorActionTargetKind::Task,
                level: task_level(attention.level),
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Complete {}", task.title),
                summary: "Mark execution complete and move the task to review.".to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if relationship_summary
            .is_some_and(|summary| summary.blocker_count > 0 || summary.blocking_count > 0)
        {
            actions.push(OperatorAction {
                action_id: format!("task:{}:resolve_dependency", task.task_id),
                kind: OperatorActionKind::ResolveDependency,
                target_kind: OperatorActionTargetKind::Task,
                level: attention.level,
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Resolve dependency for {}", task.title),
                summary:
                    "Task still has dependency edges that may need to be removed or rewritten."
                        .to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if task.status == TaskStatus::Blocked
            && relationship_summary.is_some_and(|summary| summary.blocker_count == 0)
        {
            actions.push(OperatorAction {
                action_id: format!("task:{}:reopen", task.task_id),
                kind: OperatorActionKind::ReopenBlockedTaskWhenUnblocked,
                target_kind: OperatorActionTargetKind::Task,
                level: attention.level,
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Reopen {}", task.title),
                summary:
                    "Task is blocked without remaining dependency blockers and can be reopened."
                        .to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if relationship_summary.is_some_and(|summary| summary.follow_up_child_count > 0) {
            actions.push(OperatorAction {
                action_id: format!("task:{}:promote_follow_up", task.task_id),
                kind: OperatorActionKind::PromoteFollowUp,
                target_kind: OperatorActionTargetKind::Task,
                level: if attention.level == AttentionLevel::Normal {
                    AttentionLevel::NeedsAttention
                } else {
                    attention.level
                },
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Promote follow-up on {}", task.title),
                summary: "Promote a follow-up task out of the current task chain.".to_string(),
                due_at: None,
                expires_at: None,
            });
        }

        if relationship_summary.is_some_and(|summary| {
            summary.follow_up_child_count > 0 && summary.open_follow_up_child_count == 0
        }) {
            actions.push(OperatorAction {
                action_id: format!("task:{}:close_follow_up_chain", task.task_id),
                kind: OperatorActionKind::CloseFollowUpChain,
                target_kind: OperatorActionTargetKind::Task,
                level: if attention.level == AttentionLevel::Normal {
                    AttentionLevel::NeedsAttention
                } else {
                    attention.level
                },
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Close follow-up chain for {}", task.title),
                summary: "Detach resolved follow-up tasks from this task chain.".to_string(),
                due_at: None,
                expires_at: None,
            });
        }
    }

    for handoff in handoffs
        .iter()
        .filter(|handoff| handoff.status == crate::models::HandoffStatus::Open)
    {
        let Some(attention) = handoff_attention_by_id.get(handoff.handoff_id.as_str()) else {
            continue;
        };

        match attention.freshness {
            Freshness::Aging => actions.push(OperatorAction {
                action_id: format!("handoff:{}:follow_up", handoff.handoff_id),
                kind: OperatorActionKind::FollowUpHandoff,
                target_kind: OperatorActionTargetKind::Handoff,
                level: attention.level,
                task_id: Some(handoff.task_id.clone()),
                handoff_id: Some(handoff.handoff_id.clone()),
                agent_id: Some(handoff.to_agent_id.clone()),
                title: format!("Follow up handoff {}", handoff.handoff_id),
                summary: handoff.summary.clone(),
                due_at: handoff.due_at.clone(),
                expires_at: handoff.expires_at.clone(),
            }),
            Freshness::Stale => actions.push(OperatorAction {
                action_id: format!("handoff:{}:expire", handoff.handoff_id),
                kind: OperatorActionKind::ExpireHandoff,
                target_kind: OperatorActionTargetKind::Handoff,
                level: attention.level,
                task_id: Some(handoff.task_id.clone()),
                handoff_id: Some(handoff.handoff_id.clone()),
                agent_id: Some(handoff.to_agent_id.clone()),
                title: format!("Resolve expired handoff {}", handoff.handoff_id),
                summary: handoff.summary.clone(),
                due_at: handoff.due_at.clone(),
                expires_at: handoff.expires_at.clone(),
            }),
            Freshness::Fresh | Freshness::Missing => {}
        }
    }

    actions
}

#[allow(clippy::too_many_arguments)]
fn derive_allowed_actions(
    task: &Task,
    attention: &TaskAttention,
    deadline_summary: &TaskDeadlineSummary,
    relationship_summary: &TaskRelationshipSummary,
    execution_summary: &TaskExecutionSummary,
    handoffs: &[Handoff],
    handoff_attention: &[HandoffAttention],
    now: OffsetDateTime,
) -> Vec<OperatorAction> {
    let mut actions = derive_allowed_task_actions(
        task,
        attention,
        deadline_summary,
        relationship_summary,
        execution_summary,
    );
    actions.extend(derive_allowed_handoff_actions(
        handoffs,
        handoff_attention,
        now,
    ));
    actions
}

#[allow(clippy::too_many_lines)]
fn derive_allowed_task_actions(
    task: &Task,
    attention: &TaskAttention,
    deadline_summary: &TaskDeadlineSummary,
    relationship_summary: &TaskRelationshipSummary,
    execution_summary: &TaskExecutionSummary,
) -> Vec<OperatorAction> {
    let task_level = if attention.level == AttentionLevel::Normal {
        AttentionLevel::NeedsAttention
    } else {
        attention.level
    };

    if matches!(
        task.status,
        TaskStatus::Completed | TaskStatus::Closed | TaskStatus::Cancelled
    ) {
        return Vec::new();
    }

    let mut actions = vec![
        make_task_allowed_action(
            task,
            if attention.acknowledged {
                OperatorActionKind::UnacknowledgeTask
            } else {
                OperatorActionKind::AcknowledgeTask
            },
            task_level,
            if attention.acknowledged {
                "unacknowledge"
            } else {
                "acknowledge"
            },
            if attention.acknowledged {
                format!("Unacknowledge {}", task.title)
            } else {
                format!("Acknowledge {}", task.title)
            },
            "Update operator acknowledgment for this task.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::ReassignTask,
            task_level,
            "reassign",
            format!("Reassign {}", task.title),
            "Transfer task ownership to another agent.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::ClaimTask,
            task_level,
            "claim",
            format!("Claim {}", task.title),
            "Claim this unowned task for agent execution.",
        ),
        make_task_allowed_action(
            task,
            if execution_summary.run_count > 0 {
                OperatorActionKind::ResumeTask
            } else {
                OperatorActionKind::StartTask
            },
            task_level,
            if execution_summary.run_count > 0 {
                "resume"
            } else {
                "start"
            },
            if execution_summary.run_count > 0 {
                format!("Resume {}", task.title)
            } else {
                format!("Start {}", task.title)
            },
            if execution_summary.run_count > 0 {
                "Resume execution on this previously started task."
            } else {
                "Move this claimed task into active execution."
            },
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::PauseTask,
            task_level,
            "pause",
            format!("Pause {}", task.title),
            "Pause active execution while retaining ownership.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::YieldTask,
            task_level,
            "yield",
            format!("Yield {}", task.title),
            "Release this task back to the unclaimed queue.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::CompleteTask,
            task_level,
            "complete",
            format!("Complete {}", task.title),
            "Mark execution complete and move this task to review.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::SetTaskPriority,
            task_level,
            "set_priority",
            format!("Set priority for {}", task.title),
            "Adjust task priority for the operator queue.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::SetTaskSeverity,
            task_level,
            "set_severity",
            format!("Set severity for {}", task.title),
            "Adjust task severity for triage and reporting.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::UpdateTaskNote,
            task_level,
            "note",
            format!("Update note for {}", task.title),
            "Add or clear operator context on the task.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::SetTaskDueAt,
            task_level,
            "set_task_due_at",
            format!("Set execution due date for {}", task.title),
            "Set or revise the execution deadline for this task.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::ClearTaskDueAt,
            task_level,
            "clear_task_due_at",
            format!("Clear execution due date for {}", task.title),
            "Clear the execution deadline for this task.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::SetReviewDueAt,
            task_level,
            "set_review_due_at",
            format!("Set review due date for {}", task.title),
            "Set or revise the review deadline for this task.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::ClearReviewDueAt,
            task_level,
            "clear_review_due_at",
            format!("Clear review due date for {}", task.title),
            "Clear the review deadline for this task.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::CreateHandoff,
            task_level,
            "create_handoff",
            format!("Create handoff for {}", task.title),
            "Create a new handoff from the operator console.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::PostCouncilMessage,
            task_level,
            "post_council_message",
            format!("Post council message for {}", task.title),
            "Post a new council message on the task thread.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::AttachEvidence,
            task_level,
            "attach_evidence",
            format!("Attach evidence to {}", task.title),
            "Attach supporting evidence and navigation context to the task.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::CreateFollowUpTask,
            task_level,
            "create_follow_up_task",
            format!("Create follow-up task for {}", task.title),
            "Create a follow-up task in the same project from this task detail.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::LinkTaskDependency,
            task_level,
            "link_task_dependency",
            format!("Link dependency for {}", task.title),
            "Link this task to another task as blocking or blocked-by.",
        ),
        make_task_allowed_action(
            task,
            if task.status == TaskStatus::Blocked {
                OperatorActionKind::UnblockTask
            } else {
                OperatorActionKind::BlockTask
            },
            task_level,
            if task.status == TaskStatus::Blocked {
                "unblock"
            } else {
                "block"
            },
            if task.status == TaskStatus::Blocked {
                format!("Unblock {}", task.title)
            } else {
                format!("Block {}", task.title)
            },
            "Update task lifecycle state for operator triage.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::RecordDecision,
            task_level,
            "record_decision",
            format!("Record decision for {}", task.title),
            "Persist the current-cycle review decision before closeout.",
        ),
        make_task_allowed_action(
            task,
            OperatorActionKind::CloseTask,
            task_level,
            "close_task",
            format!("Close {}", task.title),
            "Finalize review closeout and mark the task complete.",
        ),
    ];

    actions.retain(|action| match action.kind {
        OperatorActionKind::ClaimTask => {
            task.status == TaskStatus::Open
                && task.owner_agent_id.is_none()
                && relationship_summary.active_blocker_count == 0
        }
        OperatorActionKind::StartTask => {
            task.status == TaskStatus::Assigned
                && task.owner_agent_id.is_some()
                && execution_summary.run_count == 0
        }
        OperatorActionKind::ResumeTask => {
            task.status == TaskStatus::Assigned
                && task.owner_agent_id.is_some()
                && execution_summary.run_count > 0
        }
        OperatorActionKind::SetTaskDueAt => {
            is_open_task_status(task.status) && task.status != TaskStatus::ReviewRequired
        }
        OperatorActionKind::ClearTaskDueAt => deadline_summary.due_at.is_some(),
        OperatorActionKind::SetReviewDueAt => task.status == TaskStatus::ReviewRequired,
        OperatorActionKind::ClearReviewDueAt => deadline_summary.review_due_at.is_some(),
        OperatorActionKind::PauseTask => {
            task.status == TaskStatus::InProgress && task.owner_agent_id.is_some()
        }
        OperatorActionKind::YieldTask => {
            matches!(task.status, TaskStatus::Assigned | TaskStatus::InProgress)
                && task.owner_agent_id.is_some()
        }
        OperatorActionKind::CompleteTask => {
            matches!(task.status, TaskStatus::Assigned | TaskStatus::InProgress)
                && task.owner_agent_id.is_some()
                && task.status != TaskStatus::Blocked
        }
        OperatorActionKind::RecordDecision => {
            task.status == TaskStatus::ReviewRequired
                && task.verification_state == VerificationState::Pending
                && attention
                    .reasons
                    .contains(&TaskAttentionReason::ReviewReadyForDecision)
        }
        OperatorActionKind::CloseTask => {
            task.status == TaskStatus::ReviewRequired
                && task.verification_state == VerificationState::Pending
                && attention
                    .reasons
                    .contains(&TaskAttentionReason::ReviewReadyForCloseout)
        }
        _ => true,
    });

    if task.status == TaskStatus::ReviewRequired
        || matches!(
            task.verification_state,
            VerificationState::Pending | VerificationState::Failed
        )
    {
        actions.push(make_task_allowed_action(
            task,
            OperatorActionKind::VerifyTask,
            task_level,
            "verify",
            format!("Review {}", task.title),
            "Record a non-terminal review outcome and keep the task in review.",
        ));
    }

    if relationship_summary.blocker_count > 0 || relationship_summary.blocking_count > 0 {
        actions.push(make_task_allowed_action(
            task,
            OperatorActionKind::ResolveDependency,
            task_level,
            "resolve_dependency",
            format!("Resolve dependency for {}", task.title),
            "Remove an existing blocker relationship from this task graph.",
        ));
    }

    if task.status == TaskStatus::Blocked && relationship_summary.blocker_count == 0 {
        actions.push(make_task_allowed_action(
            task,
            OperatorActionKind::ReopenBlockedTaskWhenUnblocked,
            task_level,
            "reopen_when_unblocked",
            format!("Reopen {}", task.title),
            "Reopen a blocked task after its dependency blockers are cleared.",
        ));
    }

    if relationship_summary.follow_up_child_count > 0 {
        actions.push(make_task_allowed_action(
            task,
            OperatorActionKind::PromoteFollowUp,
            task_level,
            "promote_follow_up",
            format!("Promote follow-up on {}", task.title),
            "Detach one follow-up task from the current chain.",
        ));
    }

    if relationship_summary.follow_up_child_count > 0
        && relationship_summary.open_follow_up_child_count == 0
    {
        actions.push(make_task_allowed_action(
            task,
            OperatorActionKind::CloseFollowUpChain,
            task_level,
            "close_follow_up_chain",
            format!("Close follow-up chain for {}", task.title),
            "Detach resolved follow-up tasks from this task chain.",
        ));
    }

    actions
}

#[allow(clippy::too_many_lines)]
fn derive_allowed_handoff_actions(
    handoffs: &[Handoff],
    handoff_attention: &[HandoffAttention],
    now: OffsetDateTime,
) -> Vec<OperatorAction> {
    let handoff_attention_by_id: HashMap<_, _> = handoff_attention
        .iter()
        .map(|item| (item.handoff_id.as_str(), item))
        .collect();

    let mut actions = Vec::new();
    for handoff in handoffs
        .iter()
        .filter(|handoff| handoff.status == crate::models::HandoffStatus::Open)
    {
        let level = handoff_attention_by_id
            .get(handoff.handoff_id.as_str())
            .map_or(AttentionLevel::NeedsAttention, |item| item.level);
        if handoff_has_expired(handoff, now) {
            actions.push(OperatorAction {
                action_id: format!("handoff:{}:expire", handoff.handoff_id),
                kind: OperatorActionKind::ExpireHandoff,
                target_kind: OperatorActionTargetKind::Handoff,
                level,
                task_id: Some(handoff.task_id.clone()),
                handoff_id: Some(handoff.handoff_id.clone()),
                agent_id: Some(handoff.to_agent_id.clone()),
                title: format!("Expire {}", handoff.handoff_id),
                summary: "Resolve the open handoff as expired.".to_string(),
                due_at: handoff.due_at.clone(),
                expires_at: handoff.expires_at.clone(),
            });
            continue;
        }
        actions.push(OperatorAction {
            action_id: format!("handoff:{}:accept", handoff.handoff_id),
            kind: OperatorActionKind::AcceptHandoff,
            target_kind: OperatorActionTargetKind::Handoff,
            level,
            task_id: Some(handoff.task_id.clone()),
            handoff_id: Some(handoff.handoff_id.clone()),
            agent_id: Some(handoff.to_agent_id.clone()),
            title: format!("Accept {}", handoff.handoff_id),
            summary: "Accept the open handoff and record ownership or review uptake.".to_string(),
            due_at: handoff.due_at.clone(),
            expires_at: handoff.expires_at.clone(),
        });
        actions.push(OperatorAction {
            action_id: format!("handoff:{}:reject", handoff.handoff_id),
            kind: OperatorActionKind::RejectHandoff,
            target_kind: OperatorActionTargetKind::Handoff,
            level,
            task_id: Some(handoff.task_id.clone()),
            handoff_id: Some(handoff.handoff_id.clone()),
            agent_id: Some(handoff.to_agent_id.clone()),
            title: format!("Reject {}", handoff.handoff_id),
            summary: "Reject the open handoff without completing the requested action.".to_string(),
            due_at: handoff.due_at.clone(),
            expires_at: handoff.expires_at.clone(),
        });
        actions.push(OperatorAction {
            action_id: format!("handoff:{}:cancel", handoff.handoff_id),
            kind: OperatorActionKind::CancelHandoff,
            target_kind: OperatorActionTargetKind::Handoff,
            level,
            task_id: Some(handoff.task_id.clone()),
            handoff_id: Some(handoff.handoff_id.clone()),
            agent_id: Some(handoff.to_agent_id.clone()),
            title: format!("Cancel {}", handoff.handoff_id),
            summary: "Cancel the open handoff when the request is no longer needed.".to_string(),
            due_at: handoff.due_at.clone(),
            expires_at: handoff.expires_at.clone(),
        });
        actions.push(OperatorAction {
            action_id: format!("handoff:{}:complete", handoff.handoff_id),
            kind: OperatorActionKind::CompleteHandoff,
            target_kind: OperatorActionTargetKind::Handoff,
            level,
            task_id: Some(handoff.task_id.clone()),
            handoff_id: Some(handoff.handoff_id.clone()),
            agent_id: Some(handoff.to_agent_id.clone()),
            title: format!("Complete {}", handoff.handoff_id),
            summary: "Mark the open handoff as completed once the requested work lands."
                .to_string(),
            due_at: handoff.due_at.clone(),
            expires_at: handoff.expires_at.clone(),
        });
        actions.push(OperatorAction {
            action_id: format!("handoff:{}:follow_up", handoff.handoff_id),
            kind: OperatorActionKind::FollowUpHandoff,
            target_kind: OperatorActionTargetKind::Handoff,
            level,
            task_id: Some(handoff.task_id.clone()),
            handoff_id: Some(handoff.handoff_id.clone()),
            agent_id: Some(handoff.to_agent_id.clone()),
            title: format!("Follow up {}", handoff.handoff_id),
            summary: handoff.summary.clone(),
            due_at: handoff.due_at.clone(),
            expires_at: handoff.expires_at.clone(),
        });
        actions.push(OperatorAction {
            action_id: format!("handoff:{}:expire", handoff.handoff_id),
            kind: OperatorActionKind::ExpireHandoff,
            target_kind: OperatorActionTargetKind::Handoff,
            level,
            task_id: Some(handoff.task_id.clone()),
            handoff_id: Some(handoff.handoff_id.clone()),
            agent_id: Some(handoff.to_agent_id.clone()),
            title: format!("Expire {}", handoff.handoff_id),
            summary: "Resolve the open handoff as expired.".to_string(),
            due_at: handoff.due_at.clone(),
            expires_at: handoff.expires_at.clone(),
        });
    }

    actions
}

fn make_task_allowed_action(
    task: &Task,
    kind: OperatorActionKind,
    level: AttentionLevel,
    action_suffix: &str,
    title: String,
    summary: &str,
) -> OperatorAction {
    OperatorAction {
        action_id: format!("task:{}:{action_suffix}", task.task_id),
        kind,
        target_kind: OperatorActionTargetKind::Task,
        level,
        task_id: Some(task.task_id.clone()),
        handoff_id: None,
        agent_id: task.owner_agent_id.clone(),
        title,
        summary: summary.to_string(),
        due_at: None,
        expires_at: None,
    }
}

fn handoff_has_expired(handoff: &Handoff, now: OffsetDateTime) -> bool {
    handoff
        .expires_at
        .as_deref()
        .and_then(parse_timestamp)
        .is_some_and(|expires_at| expires_at <= now)
}

fn derive_pending_handoff_acceptance_task_ids(
    handoffs: &[Handoff],
    now: OffsetDateTime,
) -> HashSet<String> {
    handoffs
        .iter()
        .filter(|handoff| {
            handoff.status == crate::models::HandoffStatus::Open
                && !handoff_has_expired(handoff, now)
        })
        .map(|handoff| handoff.task_id.clone())
        .collect()
}

fn derive_pending_handoff_acceptance_task_ids_with_freshness(
    handoffs: &[Handoff],
    now: OffsetDateTime,
    freshness: Freshness,
) -> HashSet<String> {
    handoffs
        .iter()
        .filter(|handoff| {
            handoff.status == crate::models::HandoffStatus::Open
                && !handoff_has_expired(handoff, now)
                && handoff_freshness(handoff, now) == freshness
        })
        .map(|handoff| handoff.task_id.clone())
        .collect()
}

fn derive_accepted_handoff_follow_through_task_ids(
    tasks: &[Task],
    handoffs: &[Handoff],
    execution_summaries: &[TaskExecutionSummary],
) -> HashSet<String> {
    derive_accepted_handoff_follow_through_task_ids_inner(
        tasks,
        handoffs,
        execution_summaries,
        None,
        OffsetDateTime::now_utc(),
    )
}

fn derive_accepted_handoff_follow_through_task_ids_with_freshness(
    tasks: &[Task],
    handoffs: &[Handoff],
    execution_summaries: &[TaskExecutionSummary],
    now: OffsetDateTime,
    freshness: Freshness,
) -> HashSet<String> {
    derive_accepted_handoff_follow_through_task_ids_inner(
        tasks,
        handoffs,
        execution_summaries,
        Some(freshness),
        now,
    )
}

fn derive_accepted_handoff_follow_through_task_ids_inner(
    tasks: &[Task],
    handoffs: &[Handoff],
    execution_summaries: &[TaskExecutionSummary],
    freshness: Option<Freshness>,
    now: OffsetDateTime,
) -> HashSet<String> {
    let tasks_by_id: HashMap<_, _> = tasks
        .iter()
        .map(|task| (task.task_id.as_str(), task))
        .collect();
    let execution_by_task_id: HashMap<_, _> = execution_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();

    handoffs
        .iter()
        .filter(|handoff| handoff.status == crate::models::HandoffStatus::Accepted)
        .filter(|handoff| {
            freshness.is_none_or(|freshness| handoff_freshness(handoff, now) == freshness)
        })
        .filter_map(|handoff| {
            let task = tasks_by_id.get(handoff.task_id.as_str())?;
            if task.status != TaskStatus::Assigned {
                return None;
            }
            if task.owner_agent_id.as_deref() != Some(handoff.to_agent_id.as_str()) {
                return None;
            }
            let resolved_at = handoff.resolved_at.as_deref().and_then(parse_timestamp)?;
            let last_execution_after_acceptance = execution_by_task_id
                .get(handoff.task_id.as_str())
                .and_then(|summary| summary.last_execution_at.as_deref())
                .and_then(parse_timestamp)
                .is_some_and(|last_execution_at| last_execution_at >= resolved_at);

            if last_execution_after_acceptance {
                None
            } else {
                Some(handoff.task_id.clone())
            }
        })
        .collect()
}

fn derive_paused_resumable_task_ids(
    tasks: &[Task],
    execution_summaries: &[TaskExecutionSummary],
    accepted_handoff_follow_through_task_ids: &HashSet<String>,
) -> HashSet<String> {
    let execution_by_task_id: HashMap<_, _> = execution_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();

    tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::Assigned
                && task.owner_agent_id.is_some()
                && !accepted_handoff_follow_through_task_ids.contains(&task.task_id)
                && execution_by_task_id
                    .get(task.task_id.as_str())
                    .is_some_and(|summary| {
                        summary.run_count > 0
                            && summary.last_execution_action == Some(ExecutionActionKind::PauseTask)
                    })
        })
        .map(|task| task.task_id.clone())
        .collect()
}

fn derive_claimed_not_started_task_ids(
    tasks: &[Task],
    execution_summaries: &[TaskExecutionSummary],
    accepted_handoff_follow_through_task_ids: &HashSet<String>,
) -> HashSet<String> {
    let execution_by_task_id: HashMap<_, _> = execution_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();

    tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::Assigned
                && task.owner_agent_id.is_some()
                && !accepted_handoff_follow_through_task_ids.contains(&task.task_id)
                && execution_by_task_id
                    .get(task.task_id.as_str())
                    .is_some_and(|summary| {
                        summary.claim_count > 0
                            && summary.run_count == 0
                            && summary.last_execution_action == Some(ExecutionActionKind::ClaimTask)
                    })
        })
        .map(|task| task.task_id.clone())
        .collect()
}

fn derive_assigned_awaiting_claim_task_ids(
    tasks: &[Task],
    assignments: &[TaskAssignment],
    execution_summaries: &[TaskExecutionSummary],
    accepted_handoff_follow_through_task_ids: &HashSet<String>,
) -> HashSet<String> {
    let mut assignments_by_task: HashMap<&str, Vec<&TaskAssignment>> = HashMap::new();
    for assignment in assignments {
        assignments_by_task
            .entry(assignment.task_id.as_str())
            .or_default()
            .push(assignment);
    }
    let execution_by_task_id: HashMap<_, _> = execution_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();

    tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::Assigned
                && task.owner_agent_id.is_some()
                && !accepted_handoff_follow_through_task_ids.contains(&task.task_id)
                && assignments_by_task
                    .get(task.task_id.as_str())
                    .and_then(|history| history.last().copied())
                    .is_some_and(|last_assignment| {
                        if Some(last_assignment.assigned_to.as_str())
                            != task.owner_agent_id.as_deref()
                        {
                            return false;
                        }
                        if last_assignment.assigned_by == last_assignment.assigned_to {
                            return false;
                        }
                        let last_execution_at = execution_by_task_id
                            .get(task.task_id.as_str())
                            .and_then(|summary| summary.last_execution_at.as_deref())
                            .and_then(parse_timestamp);
                        let last_assigned_at = parse_timestamp(&last_assignment.assigned_at);

                        match (last_assigned_at, last_execution_at) {
                            (Some(assigned_at), Some(executed_at)) => assigned_at >= executed_at,
                            (Some(_) | None, None) => true,
                            (None, Some(_)) => false,
                        }
                    })
        })
        .map(|task| task.task_id.clone())
        .collect()
}

fn derive_review_with_graph_pressure_task_ids(
    tasks: &[Task],
    relationship_summaries: &[TaskRelationshipSummary],
) -> HashSet<String> {
    let relationship_summary_by_task_id: HashMap<_, _> = relationship_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();

    tasks
        .iter()
        .filter(|task| task.status == TaskStatus::ReviewRequired)
        .filter(|task| {
            relationship_summary_by_task_id
                .get(task.task_id.as_str())
                .is_some_and(|summary| {
                    summary.active_blocker_count > 0
                        || summary.stale_blocker_count > 0
                        || summary.open_follow_up_child_count > 0
                })
        })
        .map(|task| task.task_id.clone())
        .collect()
}

fn review_handoff_requires_follow_through(handoff: &Handoff, now: OffsetDateTime) -> bool {
    matches!(
        handoff.handoff_type,
        HandoffType::RequestReview | HandoffType::RequestVerification
    ) && match handoff.status {
        crate::models::HandoffStatus::Open => !handoff_has_expired(handoff, now),
        crate::models::HandoffStatus::Accepted => true,
        crate::models::HandoffStatus::Rejected
        | crate::models::HandoffStatus::Expired
        | crate::models::HandoffStatus::Cancelled
        | crate::models::HandoffStatus::Completed => false,
    }
}

fn derive_review_handoff_follow_through_task_ids(
    tasks: &[Task],
    handoffs: &[Handoff],
    now: OffsetDateTime,
) -> HashSet<String> {
    let handoff_task_ids: HashSet<_> = handoffs
        .iter()
        .filter(|handoff| review_handoff_requires_follow_through(handoff, now))
        .map(|handoff| handoff.task_id.as_str())
        .collect();

    tasks
        .iter()
        .filter(|task| task.status == TaskStatus::ReviewRequired)
        .filter(|task| handoff_task_ids.contains(task.task_id.as_str()))
        .map(|task| task.task_id.clone())
        .collect()
}

fn review_decision_requires_follow_through(handoff: &Handoff, now: OffsetDateTime) -> bool {
    matches!(
        handoff.handoff_type,
        HandoffType::RecordDecision | HandoffType::CloseTask
    ) && match handoff.status {
        crate::models::HandoffStatus::Open => !handoff_has_expired(handoff, now),
        crate::models::HandoffStatus::Accepted => true,
        crate::models::HandoffStatus::Rejected
        | crate::models::HandoffStatus::Expired
        | crate::models::HandoffStatus::Cancelled
        | crate::models::HandoffStatus::Completed => false,
    }
}

fn derive_review_decision_follow_through_task_ids(
    tasks: &[Task],
    handoffs: &[Handoff],
    now: OffsetDateTime,
) -> HashSet<String> {
    let handoff_task_ids: HashSet<_> = handoffs
        .iter()
        .filter(|handoff| review_decision_requires_follow_through(handoff, now))
        .map(|handoff| handoff.task_id.as_str())
        .collect();

    tasks
        .iter()
        .filter(|task| task.status == TaskStatus::ReviewRequired)
        .filter(|task| handoff_task_ids.contains(task.task_id.as_str()))
        .map(|task| task.task_id.clone())
        .collect()
}

fn derive_review_awaiting_support_task_ids(
    tasks: &[Task],
    task_events: &[TaskEvent],
) -> HashSet<String> {
    let mut events_by_task_id: HashMap<&str, Vec<&TaskEvent>> = HashMap::new();
    for event in task_events {
        events_by_task_id
            .entry(event.task_id.as_str())
            .or_default()
            .push(event);
    }

    tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::ReviewRequired
                && task.verification_state == VerificationState::Pending
        })
        .filter(|task| {
            let task_events = events_by_task_id
                .get(task.task_id.as_str())
                .map_or(&[][..], Vec::as_slice);
            let context = derive_review_cycle_context(task_events.iter().copied());
            let pending_council = context.has_council_message && !context.has_council_decision;

            !context.has_evidence || pending_council
        })
        .map(|task| task.task_id.clone())
        .collect()
}

fn derive_review_ready_for_closeout_task_ids(
    tasks: &[Task],
    task_events: &[TaskEvent],
    review_with_graph_pressure_task_ids: &HashSet<String>,
    review_handoff_follow_through_task_ids: &HashSet<String>,
    review_decision_follow_through_task_ids: &HashSet<String>,
    review_awaiting_support_task_ids: &HashSet<String>,
) -> HashSet<String> {
    let mut events_by_task_id: HashMap<&str, Vec<&TaskEvent>> = HashMap::new();
    for event in task_events {
        events_by_task_id
            .entry(event.task_id.as_str())
            .or_default()
            .push(event);
    }

    tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::ReviewRequired
                && task.verification_state == VerificationState::Pending
                && !review_with_graph_pressure_task_ids.contains(&task.task_id)
                && !review_handoff_follow_through_task_ids.contains(&task.task_id)
                && !review_decision_follow_through_task_ids.contains(&task.task_id)
                && !review_awaiting_support_task_ids.contains(&task.task_id)
        })
        .filter(|task| {
            let task_events = events_by_task_id
                .get(task.task_id.as_str())
                .map_or(&[][..], Vec::as_slice);
            derive_review_cycle_context(task_events.iter().copied()).has_council_decision
        })
        .map(|task| task.task_id.clone())
        .collect()
}

fn derive_review_ready_for_decision_task_ids(
    tasks: &[Task],
    task_events: &[TaskEvent],
    review_with_graph_pressure_task_ids: &HashSet<String>,
    review_handoff_follow_through_task_ids: &HashSet<String>,
    review_decision_follow_through_task_ids: &HashSet<String>,
    review_awaiting_support_task_ids: &HashSet<String>,
) -> HashSet<String> {
    let mut events_by_task_id: HashMap<&str, Vec<&TaskEvent>> = HashMap::new();
    for event in task_events {
        events_by_task_id
            .entry(event.task_id.as_str())
            .or_default()
            .push(event);
    }

    tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::ReviewRequired
                && task.verification_state == VerificationState::Pending
                && !review_with_graph_pressure_task_ids.contains(&task.task_id)
                && !review_handoff_follow_through_task_ids.contains(&task.task_id)
                && !review_decision_follow_through_task_ids.contains(&task.task_id)
                && !review_awaiting_support_task_ids.contains(&task.task_id)
        })
        .filter(|task| {
            let task_events = events_by_task_id
                .get(task.task_id.as_str())
                .map_or(&[][..], Vec::as_slice);
            !derive_review_cycle_context(task_events.iter().copied()).has_council_decision
        })
        .map(|task| task.task_id.clone())
        .collect()
}

fn deadline_state(deadline_at: Option<&str>, now: OffsetDateTime) -> DeadlineState {
    let Some(deadline_at) = deadline_at else {
        return DeadlineState::None;
    };
    let Some(deadline_at) = parse_timestamp(deadline_at) else {
        return DeadlineState::None;
    };
    if deadline_at <= now {
        DeadlineState::Overdue
    } else if deadline_at <= now + time::Duration::hours(DEADLINE_SOON_HOURS) {
        DeadlineState::DueSoon
    } else {
        DeadlineState::Scheduled
    }
}

fn derive_task_deadline_summaries(tasks: &[Task], now: OffsetDateTime) -> Vec<TaskDeadlineSummary> {
    tasks
        .iter()
        .map(|task| {
            let execution_state = if matches!(
                task.status,
                TaskStatus::Open
                    | TaskStatus::Assigned
                    | TaskStatus::InProgress
                    | TaskStatus::Blocked
            ) {
                deadline_state(task.due_at.as_deref(), now)
            } else {
                DeadlineState::None
            };
            let review_state = if task.status == TaskStatus::ReviewRequired {
                deadline_state(task.review_due_at.as_deref(), now)
            } else {
                DeadlineState::None
            };

            let (active_deadline_kind, active_deadline_at, active_deadline_state) =
                if review_state != DeadlineState::None {
                    (
                        Some(TaskDeadlineKind::Review),
                        task.review_due_at.clone(),
                        review_state,
                    )
                } else if execution_state != DeadlineState::None {
                    (
                        Some(TaskDeadlineKind::Execution),
                        task.due_at.clone(),
                        execution_state,
                    )
                } else {
                    (None, None, DeadlineState::None)
                };

            let (due_in_seconds, overdue_by_seconds) = active_deadline_at
                .as_deref()
                .and_then(parse_timestamp)
                .map_or((None, None), |deadline_at| {
                    let diff = deadline_at - now;
                    if diff.is_negative() {
                        (None, Some((-diff).whole_seconds()))
                    } else {
                        (Some(diff.whole_seconds()), None)
                    }
                });

            TaskDeadlineSummary {
                task_id: task.task_id.clone(),
                due_at: task.due_at.clone(),
                review_due_at: task.review_due_at.clone(),
                execution_state,
                review_state,
                active_deadline_kind,
                active_deadline_at,
                active_deadline_state,
                due_in_seconds,
                overdue_by_seconds,
            }
        })
        .collect()
}

fn derive_agent_attention(
    agents: &[AgentRegistration],
    now: OffsetDateTime,
) -> Vec<AgentAttention> {
    agents
        .iter()
        .map(|agent| {
            let freshness = heartbeat_freshness(agent.heartbeat_at.as_deref(), now);
            let mut reasons = Vec::new();
            match freshness {
                Freshness::Aging => reasons.push(AgentAttentionReason::AgingHeartbeat),
                Freshness::Stale => reasons.push(AgentAttentionReason::StaleHeartbeat),
                Freshness::Missing => reasons.push(AgentAttentionReason::MissingHeartbeat),
                Freshness::Fresh => {}
            }
            match agent.status {
                crate::models::AgentStatus::Blocked => {
                    reasons.push(AgentAttentionReason::BlockedStatus);
                }
                crate::models::AgentStatus::ReviewRequired => {
                    reasons.push(AgentAttentionReason::ReviewRequiredStatus);
                }
                crate::models::AgentStatus::Idle
                | crate::models::AgentStatus::Assigned
                | crate::models::AgentStatus::InProgress => {}
            }

            let level = if reasons.iter().any(|reason| {
                matches!(
                    reason,
                    AgentAttentionReason::StaleHeartbeat
                        | AgentAttentionReason::MissingHeartbeat
                        | AgentAttentionReason::BlockedStatus
                )
            }) {
                AttentionLevel::Critical
            } else if reasons.is_empty() {
                AttentionLevel::Normal
            } else {
                AttentionLevel::NeedsAttention
            };

            AgentAttention {
                agent_id: agent.agent_id.clone(),
                level,
                freshness,
                last_heartbeat_at: agent.heartbeat_at.clone(),
                current_task_id: agent.current_task_id.clone(),
                reasons,
            }
        })
        .collect()
}

fn derive_handoff_attention(handoffs: &[Handoff], now: OffsetDateTime) -> Vec<HandoffAttention> {
    handoffs
        .iter()
        .map(|handoff| {
            if handoff.status != crate::models::HandoffStatus::Open {
                return HandoffAttention {
                    handoff_id: handoff.handoff_id.clone(),
                    task_id: handoff.task_id.clone(),
                    level: AttentionLevel::Normal,
                    freshness: Freshness::Fresh,
                    reasons: Vec::new(),
                };
            }

            let freshness = handoff_freshness(handoff, now);
            let reasons = match freshness {
                Freshness::Aging => vec![HandoffAttentionReason::AgingOpenHandoff],
                Freshness::Stale => vec![HandoffAttentionReason::StaleOpenHandoff],
                Freshness::Fresh | Freshness::Missing => Vec::new(),
            };
            let level = if freshness == Freshness::Stale {
                AttentionLevel::Critical
            } else if freshness == Freshness::Aging {
                AttentionLevel::NeedsAttention
            } else {
                AttentionLevel::Normal
            };

            HandoffAttention {
                handoff_id: handoff.handoff_id.clone(),
                task_id: handoff.task_id.clone(),
                level,
                freshness,
                reasons,
            }
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
#[allow(clippy::too_many_arguments)]
fn derive_task_attention(
    tasks: &[Task],
    deadline_summaries: &[TaskDeadlineSummary],
    handoffs: &[Handoff],
    agent_attention: &[AgentAttention],
    relationship_summaries: &[TaskRelationshipSummary],
    assigned_awaiting_claim_task_ids: &HashSet<String>,
    review_with_graph_pressure_task_ids: &HashSet<String>,
    review_handoff_follow_through_task_ids: &HashSet<String>,
    review_decision_follow_through_task_ids: &HashSet<String>,
    review_awaiting_support_task_ids: &HashSet<String>,
    review_ready_for_decision_task_ids: &HashSet<String>,
    review_ready_for_closeout_task_ids: &HashSet<String>,
    claimed_not_started_task_ids: &HashSet<String>,
    paused_resumable_task_ids: &HashSet<String>,
    accepted_handoff_follow_through_task_ids: &HashSet<String>,
    now: OffsetDateTime,
) -> Vec<TaskAttention> {
    let deadline_summary_by_task_id: HashMap<_, _> = deadline_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();
    let agent_attention_by_id: HashMap<_, _> = agent_attention
        .iter()
        .map(|attention| (attention.agent_id.as_str(), attention))
        .collect();
    let relationship_summary_by_task_id: HashMap<_, _> = relationship_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();
    let pending_handoff_acceptance_task_ids =
        derive_pending_handoff_acceptance_task_ids(handoffs, now);
    let mut handoff_freshness_by_task: HashMap<&str, Freshness> = HashMap::new();

    for handoff in handoffs
        .iter()
        .filter(|handoff| handoff.status == crate::models::HandoffStatus::Open)
    {
        let freshness = handoff_freshness(handoff, now);
        handoff_freshness_by_task
            .entry(handoff.task_id.as_str())
            .and_modify(|current| *current = max_freshness(*current, freshness))
            .or_insert(freshness);
    }

    tasks
        .iter()
        .map(|task| {
            let is_open = is_open_task_status(task.status);
            let freshness = if is_open {
                timestamp_freshness(&task.updated_at, now, TASK_AGING_HOURS, TASK_STALE_HOURS)
            } else {
                Freshness::Fresh
            };
            let relationship_summary = relationship_summary_by_task_id
                .get(task.task_id.as_str())
                .copied();
            let deadline_summary = deadline_summary_by_task_id
                .get(task.task_id.as_str())
                .copied();
            let owner_heartbeat_freshness = task.owner_agent_id.as_deref().and_then(|owner| {
                agent_attention_by_id
                    .get(owner)
                    .map(|attention| attention.freshness)
            });
            let open_handoff_freshness = handoff_freshness_by_task
                .get(task.task_id.as_str())
                .copied();
            let acknowledged = task.acknowledged_at.is_some();

            let mut reasons = Vec::new();
            if task.status == TaskStatus::Blocked {
                reasons.push(TaskAttentionReason::Blocked);
                if let Some(summary) = relationship_summary {
                    if summary.active_blocker_count > 0 {
                        reasons.push(TaskAttentionReason::BlockedByActiveDependency);
                    }
                    if summary.stale_blocker_count > 0 {
                        reasons.push(TaskAttentionReason::BlockedByStaleDependency);
                    }
                }
            }
            if deadline_summary
                .is_some_and(|summary| summary.execution_state == DeadlineState::DueSoon)
            {
                reasons.push(TaskAttentionReason::DueSoonExecution);
            }
            if deadline_summary
                .is_some_and(|summary| summary.execution_state == DeadlineState::Overdue)
            {
                reasons.push(TaskAttentionReason::OverdueExecution);
            }
            if deadline_summary
                .is_some_and(|summary| summary.review_state == DeadlineState::DueSoon)
            {
                reasons.push(TaskAttentionReason::DueSoonReview);
            }
            if deadline_summary
                .is_some_and(|summary| summary.review_state == DeadlineState::Overdue)
            {
                reasons.push(TaskAttentionReason::OverdueReview);
            }
            if task.status == TaskStatus::ReviewRequired {
                reasons.push(TaskAttentionReason::ReviewRequired);
            }
            if review_with_graph_pressure_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::ReviewWithGraphPressure);
            }
            if review_handoff_follow_through_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::ReviewHandoffFollowThrough);
            }
            if review_decision_follow_through_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::ReviewDecisionFollowThrough);
            }
            if review_awaiting_support_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::ReviewAwaitingSupport);
            }
            if review_ready_for_decision_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::ReviewReadyForDecision);
            }
            if review_ready_for_closeout_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::ReviewReadyForCloseout);
            }
            if task.verification_state == VerificationState::Failed {
                reasons.push(TaskAttentionReason::VerificationFailed);
            }
            if relationship_summary.is_some_and(|summary| summary.open_follow_up_child_count > 0) {
                reasons.push(TaskAttentionReason::HasOpenFollowUps);
            }
            if assigned_awaiting_claim_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::AssignedAwaitingClaim);
            }
            if claimed_not_started_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::ClaimedNotStarted);
            }
            if paused_resumable_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::PausedResumable);
            }
            if pending_handoff_acceptance_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::AwaitingHandoffAcceptance);
            }
            if accepted_handoff_follow_through_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::AcceptedHandoffPendingExecution);
            }
            match task.priority {
                TaskPriority::High => reasons.push(TaskAttentionReason::HighPriority),
                TaskPriority::Critical => reasons.push(TaskAttentionReason::CriticalPriority),
                TaskPriority::Low | TaskPriority::Medium => {}
            }
            match task.severity {
                TaskSeverity::High => reasons.push(TaskAttentionReason::HighSeverity),
                TaskSeverity::Critical => reasons.push(TaskAttentionReason::CriticalSeverity),
                TaskSeverity::None | TaskSeverity::Low | TaskSeverity::Medium => {}
            }
            match freshness {
                Freshness::Aging => reasons.push(TaskAttentionReason::AgingUpdate),
                Freshness::Stale => reasons.push(TaskAttentionReason::StaleUpdate),
                Freshness::Fresh | Freshness::Missing => {}
            }
            match owner_heartbeat_freshness {
                Some(Freshness::Aging) => reasons.push(TaskAttentionReason::AgingOwnerHeartbeat),
                Some(Freshness::Stale) => reasons.push(TaskAttentionReason::StaleOwnerHeartbeat),
                Some(Freshness::Missing) => {
                    reasons.push(TaskAttentionReason::MissingOwnerHeartbeat);
                }
                Some(Freshness::Fresh) | None => {}
            }
            match open_handoff_freshness {
                Some(Freshness::Aging) => reasons.push(TaskAttentionReason::AgingOpenHandoff),
                Some(Freshness::Stale) => reasons.push(TaskAttentionReason::StaleOpenHandoff),
                Some(Freshness::Fresh | Freshness::Missing) | None => {}
            }
            if !acknowledged
                && (task_priority_rank(task.priority) >= task_priority_rank(TaskPriority::High)
                    || task_severity_rank(task.severity) >= task_severity_rank(TaskSeverity::High)
                    || !reasons.is_empty())
            {
                reasons.push(TaskAttentionReason::Unacknowledged);
            }

            let level = if reasons.iter().any(|reason| {
                matches!(
                    reason,
                    TaskAttentionReason::Blocked
                        | TaskAttentionReason::BlockedByStaleDependency
                        | TaskAttentionReason::OverdueExecution
                        | TaskAttentionReason::OverdueReview
                        | TaskAttentionReason::CriticalPriority
                        | TaskAttentionReason::CriticalSeverity
                        | TaskAttentionReason::VerificationFailed
                        | TaskAttentionReason::StaleUpdate
                        | TaskAttentionReason::StaleOwnerHeartbeat
                        | TaskAttentionReason::MissingOwnerHeartbeat
                        | TaskAttentionReason::StaleOpenHandoff
                )
            }) || (reasons.contains(&TaskAttentionReason::Unacknowledged)
                && task_priority_rank(task.priority) >= task_priority_rank(TaskPriority::Critical))
                || (reasons.contains(&TaskAttentionReason::Unacknowledged)
                    && task_severity_rank(task.severity)
                        >= task_severity_rank(TaskSeverity::Critical))
            {
                AttentionLevel::Critical
            } else if reasons.is_empty() {
                AttentionLevel::Normal
            } else {
                AttentionLevel::NeedsAttention
            };

            TaskAttention {
                task_id: task.task_id.clone(),
                level,
                freshness,
                acknowledged,
                owner_heartbeat_freshness,
                open_handoff_freshness,
                reasons,
            }
        })
        .collect()
}

fn summarize_attention(
    task_attention: &[TaskAttention],
    handoff_attention: &[HandoffAttention],
    agent_attention: &[AgentAttention],
    operator_actions: &[OperatorAction],
) -> SnapshotAttentionSummary {
    let actionable_task_count = operator_actions
        .iter()
        .filter_map(|action| action.task_id.as_deref())
        .collect::<HashSet<_>>()
        .len();
    let actionable_handoff_count = operator_actions
        .iter()
        .filter_map(|action| action.handoff_id.as_deref())
        .collect::<HashSet<_>>()
        .len();

    SnapshotAttentionSummary {
        tasks_needing_attention: task_attention
            .iter()
            .filter(|attention| attention.level != AttentionLevel::Normal)
            .count(),
        critical_tasks: task_attention
            .iter()
            .filter(|attention| attention.level == AttentionLevel::Critical)
            .count(),
        handoffs_needing_attention: handoff_attention
            .iter()
            .filter(|attention| attention.level != AttentionLevel::Normal)
            .count(),
        stale_handoffs: handoff_attention
            .iter()
            .filter(|attention| attention.freshness == Freshness::Stale)
            .count(),
        agents_needing_attention: agent_attention
            .iter()
            .filter(|attention| attention.level != AttentionLevel::Normal)
            .count(),
        stale_agents: agent_attention
            .iter()
            .filter(|attention| attention.freshness == Freshness::Stale)
            .count(),
        actionable_tasks: actionable_task_count,
        actionable_handoffs: actionable_handoff_count,
    }
}

fn heartbeat_matches_tasks(
    current_task_id: Option<&str>,
    related_task_id: Option<&str>,
    task_ids: &HashSet<String>,
) -> bool {
    match (current_task_id, related_task_id) {
        (None, None) => false,
        (Some(current), None) => task_ids.contains(current),
        (None, Some(related)) => task_ids.contains(related),
        (Some(current), Some(related)) => task_ids.contains(current) || task_ids.contains(related),
    }
}

fn handoff_freshness(handoff: &Handoff, now: OffsetDateTime) -> Freshness {
    if let Some(expires_at) = handoff.expires_at.as_deref() {
        let Some(expires_at) = parse_timestamp(expires_at) else {
            return Freshness::Missing;
        };
        if expires_at <= now {
            return Freshness::Stale;
        }
    }

    if let Some(due_at) = handoff.due_at.as_deref() {
        let Some(due_at) = parse_timestamp(due_at) else {
            return Freshness::Missing;
        };
        let minutes_until_due = (due_at - now).whole_minutes();
        if minutes_until_due <= 0 {
            return Freshness::Stale;
        }
        if minutes_until_due <= (HANDOFF_AGING_HOURS * 60) {
            return Freshness::Aging;
        }
        return Freshness::Fresh;
    }

    timestamp_freshness(
        &handoff.created_at,
        now,
        HANDOFF_AGING_HOURS,
        HANDOFF_STALE_HOURS,
    )
}

fn timestamp_freshness(
    timestamp: &str,
    now: OffsetDateTime,
    aging_hours: i64,
    stale_hours: i64,
) -> Freshness {
    let Some(parsed) = parse_timestamp(timestamp) else {
        return Freshness::Missing;
    };
    let elapsed_hours = (now - parsed).whole_hours();
    if elapsed_hours >= stale_hours {
        Freshness::Stale
    } else if elapsed_hours >= aging_hours {
        Freshness::Aging
    } else {
        Freshness::Fresh
    }
}

fn heartbeat_freshness(timestamp: Option<&str>, now: OffsetDateTime) -> Freshness {
    let Some(timestamp) = timestamp else {
        return Freshness::Missing;
    };
    let Some(parsed) = parse_timestamp(timestamp) else {
        return Freshness::Missing;
    };
    let elapsed_minutes = (now - parsed).whole_minutes();
    if elapsed_minutes >= HEARTBEAT_STALE_MINUTES {
        Freshness::Stale
    } else if elapsed_minutes >= HEARTBEAT_AGING_MINUTES {
        Freshness::Aging
    } else {
        Freshness::Fresh
    }
}

fn parse_timestamp(raw: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(raw, &Rfc3339).ok().or_else(|| {
        PrimitiveDateTime::parse(raw, SQLITE_TIMESTAMP_FORMAT)
            .ok()
            .map(PrimitiveDateTime::assume_utc)
    })
}

fn max_freshness(left: Freshness, right: Freshness) -> Freshness {
    if freshness_rank(left) >= freshness_rank(right) {
        left
    } else {
        right
    }
}

fn compare_timestamp_desc(left: &str, right: &str) -> std::cmp::Ordering {
    match (parse_timestamp(left), parse_timestamp(right)) {
        (Some(left_ts), Some(right_ts)) => right_ts.cmp(&left_ts),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => right.cmp(left),
    }
}

fn latest_heartbeat_event<'a>(
    heartbeats: &'a [&'a AgentHeartbeatEvent],
) -> Option<&'a AgentHeartbeatEvent> {
    heartbeats.iter().copied().max_by(|left, right| {
        match (
            parse_timestamp(&left.created_at),
            parse_timestamp(&right.created_at),
        ) {
            (Some(left_ts), Some(right_ts)) => left_ts.cmp(&right_ts),
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => left.created_at.cmp(&right.created_at),
        }
    })
}

fn latest_heartbeat_timestamp(heartbeats: &[&AgentHeartbeatEvent]) -> Option<String> {
    latest_heartbeat_event(heartbeats).map(|heartbeat| heartbeat.created_at.clone())
}

fn freshness_rank(freshness: Freshness) -> u8 {
    match freshness {
        Freshness::Fresh => 0,
        Freshness::Aging => 1,
        Freshness::Stale => 2,
        Freshness::Missing => 3,
    }
}

fn freshness_sort_rank(freshness: Freshness) -> u8 {
    match freshness {
        Freshness::Missing => 0,
        Freshness::Stale => 1,
        Freshness::Aging => 2,
        Freshness::Fresh => 3,
    }
}
