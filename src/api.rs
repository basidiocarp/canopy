use crate::models::{
    AgentAttention, AgentAttentionReason, AgentRegistration, ApiSnapshot, AttentionLevel,
    Freshness, Handoff, HandoffAttention, HandoffAttentionReason, SnapshotAttentionSummary, Task,
    TaskAttention, TaskAttentionReason, TaskDetail, TaskSort, TaskStatus, TaskView,
    VerificationState,
};
use crate::store::{Store, StoreError, StoreResult};
use std::collections::{HashMap, HashSet};
use time::format_description::well_known::Rfc3339;
use time::{
    OffsetDateTime, PrimitiveDateTime, format_description::FormatItem, macros::format_description,
};

const TASK_AGING_HOURS: i64 = 6;
const TASK_STALE_HOURS: i64 = 24;
const HANDOFF_AGING_HOURS: i64 = 6;
const HANDOFF_STALE_HOURS: i64 = 24;
const HEARTBEAT_AGING_MINUTES: i64 = 15;
const HEARTBEAT_STALE_MINUTES: i64 = 60;
const SQLITE_TIMESTAMP_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

#[derive(Debug, Clone, Copy)]
pub struct SnapshotOptions<'a> {
    pub project_root: Option<&'a str>,
    pub sort: TaskSort,
    pub view: TaskView,
}

impl Default for SnapshotOptions<'_> {
    fn default() -> Self {
        Self {
            project_root: None,
            sort: TaskSort::Status,
            view: TaskView::All,
        }
    }
}

/// Builds a stable read snapshot for operator surfaces.
///
/// # Errors
///
/// Returns an error if any underlying store query fails.
pub fn snapshot(store: &Store, options: SnapshotOptions<'_>) -> StoreResult<ApiSnapshot> {
    let mut agents = store.list_agents()?;
    if let Some(project_root) = options.project_root {
        agents.retain(|agent| agent.project_root == project_root);
    }

    let handoffs = store.list_handoffs(None)?;
    let mut tasks = store.list_tasks()?;
    let now = OffsetDateTime::now_utc();

    if let Some(project_root) = options.project_root {
        tasks.retain(|task| task.project_root == project_root);
    }

    let all_heartbeats = store.list_all_agent_heartbeats()?;
    let agent_attention = derive_agent_attention(&agents, now);
    let handoff_attention = derive_handoff_attention(&handoffs, now);
    let task_attention = derive_task_attention(&tasks, &handoffs, &agent_attention, now);
    let open_handoff_task_ids: HashSet<_> = handoffs
        .iter()
        .filter(|handoff| handoff.status.to_string() == "open")
        .map(|handoff| handoff.task_id.clone())
        .collect();
    let task_attention_by_id: HashMap<_, _> = task_attention
        .iter()
        .map(|attention| (attention.task_id.clone(), attention.clone()))
        .collect();

    tasks.retain(|task| {
        matches_view(
            task,
            &open_handoff_task_ids,
            task_attention_by_id.get(&task.task_id),
            options.view,
        )
    });
    sort_tasks(&mut tasks, options.sort);

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
    let filtered_handoff_attention = handoff_attention
        .into_iter()
        .filter(|attention| task_ids.contains(&attention.task_id))
        .collect::<Vec<_>>();
    let filtered_agent_attention = agent_attention
        .into_iter()
        .filter(|attention| agent_ids.contains(&attention.agent_id))
        .collect::<Vec<_>>();
    let attention = summarize_attention(
        &filtered_task_attention,
        &filtered_handoff_attention,
        &filtered_agent_attention,
    );

    Ok(ApiSnapshot {
        attention,
        agents,
        agent_attention: filtered_agent_attention,
        heartbeats,
        tasks,
        task_attention: filtered_task_attention,
        handoffs: handoffs
            .into_iter()
            .filter(|handoff| task_ids.contains(&handoff.task_id))
            .collect(),
        handoff_attention: filtered_handoff_attention,
        evidence: store
            .list_all_evidence()?
            .into_iter()
            .filter(|evidence| task_ids.contains(&evidence.task_id))
            .collect(),
    })
}

/// Builds a task-scoped read model without exposing raw tables directly.
///
/// # Errors
///
/// Returns an error if the task does not exist or any underlying store query
/// fails.
pub fn task_detail(store: &Store, task_id: &str) -> StoreResult<TaskDetail> {
    let task = store.get_task(task_id)?;
    let handoffs = store.list_handoffs(Some(task_id))?;
    let heartbeats = store.list_task_heartbeats(task_id, 25)?;
    let agents = store.list_agents()?;
    let now = OffsetDateTime::now_utc();
    let agent_attention = derive_agent_attention(&agents, now)
        .into_iter()
        .filter(|attention| {
            attention.current_task_id.as_deref() == Some(task_id)
                || task.owner_agent_id.as_deref() == Some(attention.agent_id.as_str())
                || heartbeats
                    .iter()
                    .any(|heartbeat| heartbeat.agent_id == attention.agent_id)
        })
        .collect::<Vec<_>>();
    let handoff_attention = derive_handoff_attention(&handoffs, now);
    let attention = derive_task_attention(
        std::slice::from_ref(&task),
        &handoffs,
        &agent_attention,
        now,
    )
    .into_iter()
    .next()
    .ok_or(StoreError::Validation(
        "task attention could not be derived".to_string(),
    ))?;

    Ok(TaskDetail {
        attention,
        agent_attention,
        task,
        events: store.list_task_events(task_id)?,
        heartbeats,
        handoffs,
        handoff_attention,
        messages: store.list_council_messages(task_id)?,
        evidence: store.list_evidence(task_id)?,
    })
}

fn matches_view(
    task: &Task,
    open_handoff_task_ids: &HashSet<String>,
    task_attention: Option<&TaskAttention>,
    view: TaskView,
) -> bool {
    match view {
        TaskView::All => true,
        TaskView::Active => matches!(
            task.status,
            TaskStatus::Open | TaskStatus::Assigned | TaskStatus::InProgress
        ),
        TaskView::Blocked => {
            task.status == TaskStatus::Blocked
                || task.verification_state == VerificationState::Failed
        }
        TaskView::Review => {
            task.status == TaskStatus::ReviewRequired
                || task.verification_state == VerificationState::Pending
        }
        TaskView::Handoffs => open_handoff_task_ids.contains(&task.task_id),
        TaskView::Attention => {
            task_attention.is_some_and(|attention| attention.level != AttentionLevel::Normal)
        }
    }
}

fn sort_tasks(tasks: &mut [Task], sort: TaskSort) {
    tasks.sort_by(|left, right| match sort {
        TaskSort::Title => left.title.cmp(&right.title),
        TaskSort::UpdatedAt => compare_timestamp_desc(&left.updated_at, &right.updated_at)
            .then_with(|| left.title.cmp(&right.title)),
        TaskSort::CreatedAt => compare_timestamp_desc(&left.created_at, &right.created_at)
            .then_with(|| left.title.cmp(&right.title)),
        TaskSort::Verification => verification_rank(left.verification_state)
            .cmp(&verification_rank(right.verification_state))
            .then_with(|| left.title.cmp(&right.title)),
        TaskSort::Status => status_rank(left.status)
            .cmp(&status_rank(right.status))
            .then_with(|| left.title.cmp(&right.title)),
    });
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

            let freshness = timestamp_freshness(
                &handoff.created_at,
                now,
                HANDOFF_AGING_HOURS,
                HANDOFF_STALE_HOURS,
            );
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

fn derive_task_attention(
    tasks: &[Task],
    handoffs: &[Handoff],
    agent_attention: &[AgentAttention],
    now: OffsetDateTime,
) -> Vec<TaskAttention> {
    let agent_attention_by_id: HashMap<_, _> = agent_attention
        .iter()
        .map(|attention| (attention.agent_id.as_str(), attention))
        .collect();
    let mut handoff_freshness_by_task: HashMap<&str, Freshness> = HashMap::new();

    for handoff in handoffs
        .iter()
        .filter(|handoff| handoff.status == crate::models::HandoffStatus::Open)
    {
        let freshness = timestamp_freshness(
            &handoff.created_at,
            now,
            HANDOFF_AGING_HOURS,
            HANDOFF_STALE_HOURS,
        );
        handoff_freshness_by_task
            .entry(handoff.task_id.as_str())
            .and_modify(|current| *current = max_freshness(*current, freshness))
            .or_insert(freshness);
    }

    tasks
        .iter()
        .map(|task| {
            let is_open = matches!(
                task.status,
                TaskStatus::Open
                    | TaskStatus::Assigned
                    | TaskStatus::InProgress
                    | TaskStatus::Blocked
                    | TaskStatus::ReviewRequired
            );
            let freshness = if is_open {
                timestamp_freshness(&task.updated_at, now, TASK_AGING_HOURS, TASK_STALE_HOURS)
            } else {
                Freshness::Fresh
            };
            let owner_heartbeat_freshness = task.owner_agent_id.as_deref().and_then(|owner| {
                agent_attention_by_id
                    .get(owner)
                    .map(|attention| attention.freshness)
            });
            let open_handoff_freshness = handoff_freshness_by_task
                .get(task.task_id.as_str())
                .copied();

            let mut reasons = Vec::new();
            if task.status == TaskStatus::Blocked {
                reasons.push(TaskAttentionReason::Blocked);
            }
            if task.status == TaskStatus::ReviewRequired {
                reasons.push(TaskAttentionReason::ReviewRequired);
            }
            if task.verification_state == VerificationState::Failed {
                reasons.push(TaskAttentionReason::VerificationFailed);
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

            let level = if reasons.iter().any(|reason| {
                matches!(
                    reason,
                    TaskAttentionReason::Blocked
                        | TaskAttentionReason::VerificationFailed
                        | TaskAttentionReason::StaleUpdate
                        | TaskAttentionReason::StaleOwnerHeartbeat
                        | TaskAttentionReason::MissingOwnerHeartbeat
                        | TaskAttentionReason::StaleOpenHandoff
                )
            }) {
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
) -> SnapshotAttentionSummary {
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

fn freshness_rank(freshness: Freshness) -> u8 {
    match freshness {
        Freshness::Fresh => 0,
        Freshness::Aging => 1,
        Freshness::Stale => 2,
        Freshness::Missing => 3,
    }
}
