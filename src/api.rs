use crate::models::{
    AgentAttention, AgentAttentionReason, AgentHeartbeatEvent, AgentHeartbeatSummary,
    AgentRegistration, ApiSnapshot, AttentionLevel, BreachSeverity, DeadlineState,
    ExecutionActionKind, Freshness, Handoff, HandoffAttention, HandoffAttentionReason, HandoffType,
    OperatorAction, OperatorActionKind, OperatorActionTargetKind, ReviewCycleState,
    SnapshotAttentionSummary, SnapshotPreset, SnapshotSlaSummary, Task, TaskAssignment,
    TaskAttention, TaskAttentionReason, TaskDeadlineKind, TaskDeadlineSummary, TaskDetail,
    TaskEvent, TaskEventType, TaskExecutionSummary, TaskHeartbeatSummary, TaskOwnershipSummary,
    TaskPriority, TaskQueueStatus, TaskRelationship, TaskRelationshipKind, TaskRelationshipSummary,
    TaskSeverity, TaskSlaSummary, TaskSort, TaskStatus, TaskView, TaskWorkflowContext,
    VerificationState, derive_review_cycle_context,
};
use crate::store::{CanopyStore, StoreError, StoreResult};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use std::collections::{HashMap, HashSet};

mod allowed_actions;
mod attention;
mod context;
mod operator_actions;
mod sla;
mod views;

use self::{
    allowed_actions::derive_allowed_actions,
    attention::{
        derive_agent_attention, derive_handoff_attention, derive_task_attention,
        summarize_attention,
    },
    context::{
        SnapshotContext, accepted_handoff_requires_follow_through, compare_timestamp_desc,
        freshness_sort_rank, handoff_freshness, heartbeat_freshness, heartbeat_matches_tasks,
        max_freshness, parse_timestamp, review_decision_requires_follow_through,
        review_handoff_requires_follow_through, timestamp_freshness,
    },
    operator_actions::{derive_operator_actions, handoff_has_expired, make_task_allowed_action},
    sla::{derive_task_deadline_summaries, derive_task_sla_summaries, summarize_sla},
    views::{
        derive_task_relationship_summaries, is_open_task_status, matches_filters, matches_view,
        resolve_snapshot_options, sort_tasks, task_level, task_priority_rank, task_severity_rank,
    },
};

type OffsetDateTime = DateTime<Utc>;

const TASK_AGING_HOURS: i64 = 6;
const TASK_STALE_HOURS: i64 = 24;
const DEADLINE_SOON_HOURS: i64 = 24;
const HANDOFF_AGING_HOURS: i64 = 6;
const HANDOFF_STALE_HOURS: i64 = 24;
const HEARTBEAT_AGING_MINUTES: i64 = 15;
const HEARTBEAT_STALE_MINUTES: i64 = 60;
const CANOPY_API_SCHEMA_VERSION: &str = "1.0";

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

#[allow(clippy::struct_field_names)]
struct SlaQueueSets<'a> {
    due_soon_handoff_acceptance_task_ids: &'a HashSet<String>,
    overdue_handoff_acceptance_task_ids: &'a HashSet<String>,
    due_soon_accepted_handoff_follow_through_task_ids: &'a HashSet<String>,
    overdue_accepted_handoff_follow_through_task_ids: &'a HashSet<String>,
    due_soon_review_handoff_follow_through_task_ids: &'a HashSet<String>,
    overdue_review_handoff_follow_through_task_ids: &'a HashSet<String>,
    due_soon_review_decision_follow_through_task_ids: &'a HashSet<String>,
    overdue_review_decision_follow_through_task_ids: &'a HashSet<String>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Default)]
struct OverdueTaskSlaQueues {
    handoff_acceptance: bool,
    accepted_handoff_follow_through: bool,
    review_handoff_follow_through: bool,
    review_decision_follow_through: bool,
}

/// Builds a stable read snapshot for operator surfaces.
///
/// # Errors
///
/// Returns an error if any underlying store query fails.
#[allow(clippy::too_many_lines)]
pub fn snapshot(
    store: &(impl CanopyStore + ?Sized),
    options: SnapshotOptions<'_>,
) -> StoreResult<ApiSnapshot> {
    let options = resolve_snapshot_options(options);
    let project_root = options.project_root.as_deref();

    // Load only agents, tasks, handoffs, events, assignments, relationships and
    // evidence that belong to the requested project. When no project filter is
    // set each method falls back to loading everything.
    let agents = store.list_agents_filtered(project_root)?;

    let handoffs = store.list_handoffs_for_project(project_root)?;
    let mut tasks = store.list_tasks_filtered(project_root, None, None)?;
    let now = Utc::now();

    let project_task_events = store.list_task_events_for_project(project_root)?;
    let project_assignments = store.list_task_assignments_for_project(project_root)?;
    let relationships = store.list_task_relationships_for_project(project_root)?;
    let relationship_summaries = derive_task_relationship_summaries(&tasks, &relationships, now);
    let project_execution_summaries =
        derive_task_execution_summaries(&tasks, &project_task_events, now);
    let project_evidence = store.list_evidence_for_project(project_root)?;
    let project_workflow_contexts = store.list_task_workflow_contexts(project_root)?;
    let project_deadline_summaries = derive_task_deadline_summaries(&tasks, now);
    let context = SnapshotContext::new(
        &tasks,
        &handoffs,
        &project_task_events,
        &project_assignments,
        &relationship_summaries,
        &project_execution_summaries,
        &project_workflow_contexts,
        &project_deadline_summaries,
        now,
    );

    // Heartbeats are pre-scoped to the project's agents; the .take(50) cap is
    // preserved as an explicit limit parameter.
    let all_heartbeats = store.list_agent_heartbeats_for_project(project_root, Some(50))?;
    let agent_attention = derive_agent_attention(&agents, now);
    let handoff_attention = derive_handoff_attention(&handoffs, now);
    let task_attention = derive_task_attention(&tasks, &handoffs, &agent_attention, &context);
    let sla_queue_sets = context.sla_queue_sets();
    let task_sla_summaries = derive_task_sla_summaries(
        &tasks,
        &project_deadline_summaries,
        &handoffs,
        &project_execution_summaries,
        &sla_queue_sets,
        now,
    );
    let task_attention_by_id: HashMap<_, _> = task_attention
        .iter()
        .map(|attention| (attention.task_id.clone(), attention.clone()))
        .collect();
    tasks.retain(|task| {
        matches_view(
            task,
            &context,
            task_attention_by_id.get(&task.task_id),
            options.view,
        ) && matches_filters(task, task_attention_by_id.get(&task.task_id), &options)
    });
    sort_tasks(&mut tasks, options.sort, &task_attention_by_id);

    let task_ids: HashSet<_> = tasks.iter().map(|task| task.task_id.clone()).collect();
    let agent_ids: HashSet<_> = agents.iter().map(|agent| agent.agent_id.clone()).collect();
    // all_heartbeats is already scoped to project agents; only retain those
    // associated with the visible (view-filtered) task set.
    let heartbeats = all_heartbeats
        .into_iter()
        .filter(|heartbeat| {
            heartbeat_matches_tasks(
                heartbeat.current_task_id.as_deref(),
                heartbeat.related_task_id.as_deref(),
                &task_ids,
            )
        })
        .collect::<Vec<_>>();
    let filtered_task_attention = task_attention
        .into_iter()
        .filter(|attention| task_ids.contains(&attention.task_id))
        .collect::<Vec<_>>();
    let filtered_deadline_summaries = project_deadline_summaries
        .into_iter()
        .filter(|summary| task_ids.contains(&summary.task_id))
        .collect::<Vec<_>>();
    let filtered_task_sla_summaries = task_sla_summaries
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
    let workflow_contexts = project_workflow_contexts
        .into_iter()
        .filter(|context| task_ids.contains(&context.task_id))
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
        &workflow_contexts,
    );
    let attention = summarize_attention(
        &tasks,
        &filtered_task_attention,
        &filtered_handoff_attention,
        &filtered_agent_attention,
        &operator_actions,
    );
    let sla_summary = summarize_sla(&filtered_task_sla_summaries);

    Ok(ApiSnapshot {
        schema_version: CANOPY_API_SCHEMA_VERSION.to_string(),
        attention,
        sla_summary,
        agents,
        agent_attention: filtered_agent_attention,
        agent_heartbeat_summaries,
        heartbeats,
        tasks,
        task_attention: filtered_task_attention,
        task_sla_summaries: filtered_task_sla_summaries,
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
        workflow_contexts,
    })
}

/// Builds a task-scoped read model without exposing raw tables directly.
///
/// # Errors
///
/// Returns an error if the task does not exist or any underlying store query
/// fails.
#[allow(clippy::too_many_lines)]
pub fn task_detail(store: &(impl CanopyStore + ?Sized), task_id: &str) -> StoreResult<TaskDetail> {
    let task = store.get_task(task_id)?;
    let handoffs = store.list_handoffs(Some(task_id))?;
    let assignments = store.list_task_assignments(Some(task_id))?;
    let events = store.list_task_events(task_id)?;
    let council_session = store.get_council_session(task_id)?;
    let messages = store.list_council_messages(task_id)?;
    let evidence = store.list_evidence(task_id)?;
    let workflow_context = Some(store.get_task_workflow_context(task_id)?);
    let heartbeats = store.list_task_heartbeats(task_id, 25)?;
    let agents = store.list_agents()?;
    let now = Utc::now();
    let execution_summary =
        derive_task_execution_summaries(std::slice::from_ref(&task), &events, now)
            .into_iter()
            .next()
            .ok_or(StoreError::Validation(
                "task execution summary could not be derived".to_string(),
            ))?;
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
    let deadline_summary = derive_task_deadline_summaries(std::slice::from_ref(&task), now)
        .into_iter()
        .next()
        .ok_or(StoreError::Validation(
            "task deadline summary could not be derived".to_string(),
        ))?;
    let context = SnapshotContext::new(
        std::slice::from_ref(&task),
        &handoffs,
        &events,
        &assignments,
        std::slice::from_ref(&relationship_summary),
        std::slice::from_ref(&execution_summary),
        workflow_context.as_slice(),
        std::slice::from_ref(&deadline_summary),
        now,
    );
    let sla_queue_sets = context.sla_queue_sets();
    let sla_summary = derive_task_sla_summaries(
        std::slice::from_ref(&task),
        std::slice::from_ref(&deadline_summary),
        &handoffs,
        std::slice::from_ref(&execution_summary),
        &sla_queue_sets,
        now,
    )
    .into_iter()
    .next()
    .ok_or(StoreError::Validation(
        "task SLA summary could not be derived".to_string(),
    ))?;
    let attention = derive_task_attention(
        std::slice::from_ref(&task),
        &handoffs,
        &agent_attention,
        &context,
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
    let children = store.get_children(task_id)?;
    let operator_actions = derive_operator_actions(
        std::slice::from_ref(&task),
        std::slice::from_ref(&attention),
        std::slice::from_ref(&deadline_summary),
        std::slice::from_ref(&relationship_summary),
        std::slice::from_ref(&execution_summary),
        &handoffs,
        &handoff_attention,
        workflow_context.as_slice(),
    );
    let allowed_actions = derive_allowed_actions(
        &task,
        &attention,
        &deadline_summary,
        &relationship_summary,
        &execution_summary,
        council_session.as_ref(),
        &handoffs,
        &handoff_attention,
        now,
    );

    let children_complete = relationship_summary.children_complete;
    let tool_adoption_score = store.get_tool_adoption_score(task_id)?;

    Ok(TaskDetail {
        schema_version: CANOPY_API_SCHEMA_VERSION.to_string(),
        attention,
        sla_summary,
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
        council_session,
        messages,
        evidence,
        relationships,
        relationship_summary,
        workflow_context,
        related_tasks: store.list_related_tasks(task_id)?,
        children_complete,
        children,
        parent_id: store.get_parent_id(task_id)?,
        tool_adoption_score,
    })
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
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
                    parse_timestamp(raw).map(|started_at| (now - started_at).num_seconds().max(0))
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
