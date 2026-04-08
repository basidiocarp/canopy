#![allow(clippy::wildcard_imports)]

use super::*;

#[allow(dead_code)]
pub(super) fn resolve_snapshot_options(options: SnapshotOptions<'_>) -> ResolvedSnapshotOptions {
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
pub(super) fn apply_preset(options: &mut ResolvedSnapshotOptions, preset: SnapshotPreset) {
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
        SnapshotPreset::DueSoonReviewHandoffFollowThrough => {
            options.view = TaskView::DueSoonReviewHandoffFollowThrough;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::OverdueReviewHandoffFollowThrough => {
            options.view = TaskView::OverdueReviewHandoffFollowThrough;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::DueSoonReviewDecisionFollowThrough => {
            options.view = TaskView::DueSoonReviewDecisionFollowThrough;
            options.sort = TaskSort::Attention;
        }
        SnapshotPreset::OverdueReviewDecisionFollowThrough => {
            options.view = TaskView::OverdueReviewDecisionFollowThrough;
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
        SnapshotPreset::FileConflicts => {
            options.view = TaskView::FileConflicts;
            options.sort = TaskSort::UpdatedAt;
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn matches_view(
    task: &Task,
    context: &SnapshotContext,
    task_attention: Option<&TaskAttention>,
    view: TaskView,
) -> bool {
    let deadline_summary = context.deadline_summary(&task.task_id);
    let relationship_summary = context.relationship_summary(&task.task_id);

    if let Some(matches) = matches_review_view(
        &task.task_id,
        view,
        &context.review_with_graph_pressure_task_ids,
        &context.review_handoff_follow_through_task_ids,
        &context.due_soon_review_handoff_follow_through_task_ids,
        &context.overdue_review_handoff_follow_through_task_ids,
        &context.due_soon_review_decision_follow_through_task_ids,
        &context.overdue_review_decision_follow_through_task_ids,
        &context.review_decision_follow_through_task_ids,
        &context.review_awaiting_support_task_ids,
        &context.review_ready_for_decision_task_ids,
        &context.review_ready_for_closeout_task_ids,
    ) {
        return matches;
    }

    if let Some(matches) = matches_handoff_view(
        &task.task_id,
        view,
        &context.open_handoff_task_ids,
        &context.pending_handoff_acceptance_task_ids,
        &context.due_soon_handoff_acceptance_task_ids,
        &context.overdue_handoff_acceptance_task_ids,
        &context.accepted_handoff_follow_through_task_ids,
        &context.due_soon_accepted_handoff_follow_through_task_ids,
        &context.overdue_accepted_handoff_follow_through_task_ids,
    ) {
        return matches;
    }

    if let Some(matches) = matches_deadline_view(task, deadline_summary, view) {
        return matches;
    }

    match view {
        TaskView::All => true,
        TaskView::Active => matches!(
            task.status,
            TaskStatus::Open | TaskStatus::Assigned | TaskStatus::InProgress
        ),
        TaskView::Unclaimed => task.status == TaskStatus::Open && task.owner_agent_id.is_none(),
        TaskView::AssignedAwaitingClaim => context
            .assigned_awaiting_claim_task_ids
            .contains(&task.task_id),
        TaskView::ClaimedNotStarted => context.claimed_not_started_task_ids.contains(&task.task_id),
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
        TaskView::PausedResumable => context.paused_resumable_task_ids.contains(&task.task_id),
        TaskView::DueSoon
        | TaskView::DueSoonExecution
        | TaskView::DueSoonReview
        | TaskView::OverdueExecution
        | TaskView::OverdueExecutionOwned
        | TaskView::OverdueExecutionUnclaimed
        | TaskView::OverdueReview => unreachable!("deadline views handled above"),
        TaskView::AwaitingHandoffAcceptance
        | TaskView::DueSoonHandoffAcceptance
        | TaskView::OverdueHandoffAcceptance
        | TaskView::AcceptedHandoffFollowThrough
        | TaskView::DueSoonAcceptedHandoffFollowThrough
        | TaskView::OverdueAcceptedHandoffFollowThrough
        | TaskView::Handoffs => unreachable!("handoff views handled above"),
        TaskView::Blocked => {
            task.status == TaskStatus::Blocked
                || task.verification_state == VerificationState::Failed
        }
        TaskView::BlockedByDependencies => {
            task.status == TaskStatus::Blocked
                && relationship_summary.is_some_and(|summary| summary.blocker_count > 0)
        }
        TaskView::ReviewWithGraphPressure
        | TaskView::DueSoonReviewHandoffFollowThrough
        | TaskView::OverdueReviewHandoffFollowThrough
        | TaskView::DueSoonReviewDecisionFollowThrough
        | TaskView::OverdueReviewDecisionFollowThrough
        | TaskView::ReviewHandoffFollowThrough
        | TaskView::ReviewDecisionFollowThrough
        | TaskView::ReviewAwaitingSupport
        | TaskView::ReviewReadyForDecision
        | TaskView::ReviewReadyForCloseout => unreachable!("review queue views handled above"),
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
        TaskView::FileConflicts => {
            // Include tasks that have a non-empty scope and are active
            !task.scope.is_empty()
                && matches!(task.status, TaskStatus::Assigned | TaskStatus::InProgress)
        }
    }
}

pub(super) fn matches_deadline_view(
    task: &Task,
    deadline_summary: Option<&TaskDeadlineSummary>,
    view: TaskView,
) -> Option<bool> {
    Some(match view {
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
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn matches_review_view(
    task_id: &str,
    view: TaskView,
    review_with_graph_pressure_task_ids: &HashSet<String>,
    review_handoff_follow_through_task_ids: &HashSet<String>,
    due_soon_review_handoff_follow_through_task_ids: &HashSet<String>,
    overdue_review_handoff_follow_through_task_ids: &HashSet<String>,
    due_soon_review_decision_follow_through_task_ids: &HashSet<String>,
    overdue_review_decision_follow_through_task_ids: &HashSet<String>,
    review_decision_follow_through_task_ids: &HashSet<String>,
    review_awaiting_support_task_ids: &HashSet<String>,
    review_ready_for_decision_task_ids: &HashSet<String>,
    review_ready_for_closeout_task_ids: &HashSet<String>,
) -> Option<bool> {
    Some(match view {
        TaskView::ReviewWithGraphPressure => review_with_graph_pressure_task_ids.contains(task_id),
        TaskView::DueSoonReviewHandoffFollowThrough => {
            due_soon_review_handoff_follow_through_task_ids.contains(task_id)
        }
        TaskView::OverdueReviewHandoffFollowThrough => {
            overdue_review_handoff_follow_through_task_ids.contains(task_id)
        }
        TaskView::DueSoonReviewDecisionFollowThrough => {
            due_soon_review_decision_follow_through_task_ids.contains(task_id)
        }
        TaskView::OverdueReviewDecisionFollowThrough => {
            overdue_review_decision_follow_through_task_ids.contains(task_id)
        }
        TaskView::ReviewHandoffFollowThrough => {
            review_handoff_follow_through_task_ids.contains(task_id)
        }
        TaskView::ReviewDecisionFollowThrough => {
            review_decision_follow_through_task_ids.contains(task_id)
        }
        TaskView::ReviewAwaitingSupport => review_awaiting_support_task_ids.contains(task_id),
        TaskView::ReviewReadyForDecision => review_ready_for_decision_task_ids.contains(task_id),
        TaskView::ReviewReadyForCloseout => review_ready_for_closeout_task_ids.contains(task_id),
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn matches_handoff_view(
    task_id: &str,
    view: TaskView,
    open_handoff_task_ids: &HashSet<String>,
    pending_handoff_acceptance_task_ids: &HashSet<String>,
    due_soon_handoff_acceptance_task_ids: &HashSet<String>,
    overdue_handoff_acceptance_task_ids: &HashSet<String>,
    accepted_handoff_follow_through_task_ids: &HashSet<String>,
    due_soon_accepted_handoff_follow_through_task_ids: &HashSet<String>,
    overdue_accepted_handoff_follow_through_task_ids: &HashSet<String>,
) -> Option<bool> {
    Some(match view {
        TaskView::AwaitingHandoffAcceptance => {
            pending_handoff_acceptance_task_ids.contains(task_id)
        }
        TaskView::DueSoonHandoffAcceptance => {
            due_soon_handoff_acceptance_task_ids.contains(task_id)
        }
        TaskView::OverdueHandoffAcceptance => overdue_handoff_acceptance_task_ids.contains(task_id),
        TaskView::AcceptedHandoffFollowThrough => {
            accepted_handoff_follow_through_task_ids.contains(task_id)
        }
        TaskView::DueSoonAcceptedHandoffFollowThrough => {
            due_soon_accepted_handoff_follow_through_task_ids.contains(task_id)
        }
        TaskView::OverdueAcceptedHandoffFollowThrough => {
            overdue_accepted_handoff_follow_through_task_ids.contains(task_id)
        }
        TaskView::Handoffs => open_handoff_task_ids.contains(task_id),
        _ => return None,
    })
}

pub(super) fn matches_filters(
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

pub(super) fn sort_tasks(
    tasks: &mut [Task],
    sort: TaskSort,
    task_attention: &HashMap<String, TaskAttention>,
) {
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

pub(super) fn is_open_task_status(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Open
            | TaskStatus::Assigned
            | TaskStatus::InProgress
            | TaskStatus::Blocked
            | TaskStatus::ReviewRequired
    )
}

pub(super) fn derive_task_relationship_summaries(
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
                    parent_count: 0,
                    child_count: 0,
                    open_child_count: 0,
                    children_complete: false,
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
            TaskRelationshipKind::Parent => {
                if let Some(summary) = summaries.get_mut(&relationship.source_task_id) {
                    summary.parent_count += 1;
                }
                if let Some(summary) = summaries.get_mut(&relationship.target_task_id) {
                    summary.child_count += 1;
                    if is_open_task_status(source_task.status) {
                        summary.open_child_count += 1;
                    }
                }
            }
        }
    }

    tasks
        .iter()
        .map(|task| {
            let mut summary = summaries
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
                    parent_count: 0,
                    child_count: 0,
                    open_child_count: 0,
                    children_complete: false,
                });
            summary.children_complete = summary.child_count > 0 && summary.open_child_count == 0;
            summary
        })
        .collect()
}

pub(super) fn relationship_blocker_is_stale(task: &Task, now: OffsetDateTime) -> bool {
    if !is_open_task_status(task.status) {
        return true;
    }

    timestamp_freshness(&task.updated_at, now, TASK_AGING_HOURS, TASK_STALE_HOURS)
        == Freshness::Stale
}

pub(super) fn status_rank(status: TaskStatus) -> u8 {
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

pub(super) fn verification_rank(state: VerificationState) -> u8 {
    match state {
        VerificationState::Failed => 0,
        VerificationState::Pending => 1,
        VerificationState::Unknown => 2,
        VerificationState::Passed => 3,
    }
}

pub(super) fn task_priority_rank(priority: TaskPriority) -> u8 {
    match priority {
        TaskPriority::Low => 0,
        TaskPriority::Medium => 1,
        TaskPriority::High => 2,
        TaskPriority::Critical => 3,
    }
}

pub(super) fn task_severity_rank(severity: TaskSeverity) -> u8 {
    match severity {
        TaskSeverity::None => 0,
        TaskSeverity::Low => 1,
        TaskSeverity::Medium => 2,
        TaskSeverity::High => 3,
        TaskSeverity::Critical => 4,
    }
}

pub(super) fn attention_level_rank(level: AttentionLevel) -> u8 {
    match level {
        AttentionLevel::Normal => 0,
        AttentionLevel::NeedsAttention => 1,
        AttentionLevel::Critical => 2,
    }
}

pub(super) fn task_level(level: AttentionLevel) -> AttentionLevel {
    if level == AttentionLevel::Normal {
        AttentionLevel::NeedsAttention
    } else {
        level
    }
}

pub(super) fn attention_sort_key(attention: Option<&TaskAttention>) -> (u8, u8, u8, u8) {
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
