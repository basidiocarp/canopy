#![allow(clippy::wildcard_imports)]

use super::*;

#[allow(dead_code)]
pub(super) fn derive_agent_attention(
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

pub(super) fn derive_handoff_attention(
    handoffs: &[Handoff],
    now: OffsetDateTime,
) -> Vec<HandoffAttention> {
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
pub(super) fn derive_task_attention(
    tasks: &[Task],
    handoffs: &[Handoff],
    agent_attention: &[AgentAttention],
    context: &SnapshotContext,
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
        let freshness = handoff_freshness(handoff, context.now);
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
                timestamp_freshness(
                    &task.updated_at,
                    context.now,
                    TASK_AGING_HOURS,
                    TASK_STALE_HOURS,
                )
            } else {
                Freshness::Fresh
            };
            let relationship_summary = context.relationship_summary(&task.task_id);
            let deadline_summary = context.deadline_summary(&task.task_id);
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
            if context
                .review_with_graph_pressure_task_ids
                .contains(&task.task_id)
            {
                reasons.push(TaskAttentionReason::ReviewWithGraphPressure);
            }
            if context
                .review_handoff_follow_through_task_ids
                .contains(&task.task_id)
            {
                reasons.push(TaskAttentionReason::ReviewHandoffFollowThrough);
            }
            if context
                .review_decision_follow_through_task_ids
                .contains(&task.task_id)
            {
                reasons.push(TaskAttentionReason::ReviewDecisionFollowThrough);
            }
            if context
                .review_awaiting_support_task_ids
                .contains(&task.task_id)
            {
                reasons.push(TaskAttentionReason::ReviewAwaitingSupport);
            }
            if context
                .review_ready_for_decision_task_ids
                .contains(&task.task_id)
            {
                reasons.push(TaskAttentionReason::ReviewReadyForDecision);
            }
            if context
                .review_ready_for_closeout_task_ids
                .contains(&task.task_id)
            {
                reasons.push(TaskAttentionReason::ReviewReadyForCloseout);
            }
            if task.verification_state == VerificationState::Failed {
                reasons.push(TaskAttentionReason::VerificationFailed);
            }
            if relationship_summary.is_some_and(|summary| summary.open_follow_up_child_count > 0) {
                reasons.push(TaskAttentionReason::HasOpenFollowUps);
            }
            if is_open && relationship_summary.is_some_and(|summary| summary.children_complete) {
                reasons.push(TaskAttentionReason::AllChildrenComplete);
            }
            if context
                .assigned_awaiting_claim_task_ids
                .contains(&task.task_id)
            {
                reasons.push(TaskAttentionReason::AssignedAwaitingClaim);
            }
            if context.claimed_not_started_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::ClaimedNotStarted);
            }
            if context.paused_resumable_task_ids.contains(&task.task_id) {
                reasons.push(TaskAttentionReason::PausedResumable);
            }
            if context
                .pending_handoff_acceptance_task_ids
                .contains(&task.task_id)
            {
                reasons.push(TaskAttentionReason::AwaitingHandoffAcceptance);
            }
            if context
                .accepted_handoff_follow_through_task_ids
                .contains(&task.task_id)
            {
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

pub(super) fn summarize_attention(
    tasks: &[Task],
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
    let needs_verification_count = tasks
        .iter()
        .filter(|task| task.verification_required)
        .count();

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
        needs_verification_count,
    }
}
