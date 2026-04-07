use super::*;

#[allow(dead_code)]
pub(super) fn deadline_state(deadline_at: Option<&str>, now: OffsetDateTime) -> DeadlineState {
    let Some(deadline_at) = deadline_at else {
        return DeadlineState::None;
    };
    let Some(deadline_at) = parse_timestamp(deadline_at) else {
        return DeadlineState::None;
    };
    if deadline_at <= now {
        DeadlineState::Overdue
    } else if deadline_at <= now + Duration::hours(DEADLINE_SOON_HOURS) {
        DeadlineState::DueSoon
    } else {
        DeadlineState::Scheduled
    }
}

pub(super) fn derive_task_deadline_summaries(
    tasks: &[Task],
    now: OffsetDateTime,
) -> Vec<TaskDeadlineSummary> {
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
                    if diff < Duration::zero() {
                        (None, Some((-diff).num_seconds()))
                    } else {
                        (Some(diff.num_seconds()), None)
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

pub(super) fn derive_task_sla_summaries(
    tasks: &[Task],
    deadline_summaries: &[TaskDeadlineSummary],
    handoffs: &[Handoff],
    execution_summaries: &[TaskExecutionSummary],
    queue_sets: &SlaQueueSets<'_>,
    now: OffsetDateTime,
) -> Vec<TaskSlaSummary> {
    let deadline_summary_by_task_id: HashMap<_, _> = deadline_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();
    let execution_by_task_id: HashMap<_, _> = execution_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();
    let handoffs_by_task_id: HashMap<_, Vec<&Handoff>> =
        handoffs.iter().fold(HashMap::new(), |mut acc, handoff| {
            acc.entry(handoff.task_id.as_str())
                .or_default()
                .push(handoff);
            acc
        });

    tasks
        .iter()
        .map(|task| {
            let deadline_summary = deadline_summary_by_task_id
                .get(task.task_id.as_str())
                .copied();
            let task_handoffs = handoffs_by_task_id
                .get(task.task_id.as_str())
                .map_or(&[][..], Vec::as_slice);

            let execution_due_soon = deadline_summary
                .is_some_and(|summary| summary.execution_state == DeadlineState::DueSoon);
            let execution_overdue = deadline_summary
                .is_some_and(|summary| summary.execution_state == DeadlineState::Overdue);
            let review_due_soon = deadline_summary
                .is_some_and(|summary| summary.review_state == DeadlineState::DueSoon);
            let review_overdue = deadline_summary
                .is_some_and(|summary| summary.review_state == DeadlineState::Overdue);

            let due_soon_count = [
                execution_due_soon,
                review_due_soon,
                queue_sets
                    .due_soon_handoff_acceptance_task_ids
                    .contains(&task.task_id),
                queue_sets
                    .due_soon_accepted_handoff_follow_through_task_ids
                    .contains(&task.task_id),
                queue_sets
                    .due_soon_review_handoff_follow_through_task_ids
                    .contains(&task.task_id),
                queue_sets
                    .due_soon_review_decision_follow_through_task_ids
                    .contains(&task.task_id),
            ]
            .into_iter()
            .filter(|flag| *flag)
            .count();
            let overdue_count = [
                execution_overdue,
                review_overdue,
                queue_sets
                    .overdue_handoff_acceptance_task_ids
                    .contains(&task.task_id),
                queue_sets
                    .overdue_accepted_handoff_follow_through_task_ids
                    .contains(&task.task_id),
                queue_sets
                    .overdue_review_handoff_follow_through_task_ids
                    .contains(&task.task_id),
                queue_sets
                    .overdue_review_decision_follow_through_task_ids
                    .contains(&task.task_id),
            ]
            .into_iter()
            .filter(|flag| *flag)
            .count();

            let deadline_overdue_seconds =
                deadline_summary.and_then(|summary| summary.overdue_by_seconds);
            let overdue_queue_flags = OverdueTaskSlaQueues {
                handoff_acceptance: queue_sets
                    .overdue_handoff_acceptance_task_ids
                    .contains(&task.task_id),
                accepted_handoff_follow_through: queue_sets
                    .overdue_accepted_handoff_follow_through_task_ids
                    .contains(&task.task_id),
                review_handoff_follow_through: queue_sets
                    .overdue_review_handoff_follow_through_task_ids
                    .contains(&task.task_id),
                review_decision_follow_through: queue_sets
                    .overdue_review_decision_follow_through_task_ids
                    .contains(&task.task_id),
            };
            let handoff_overdue_seconds = task_handoffs
                .iter()
                .filter_map(|handoff| {
                    overdue_handoff_age_seconds(
                        handoff,
                        task,
                        execution_by_task_id.get(task.task_id.as_str()).copied(),
                        now,
                        overdue_queue_flags,
                    )
                })
                .max();
            let oldest_overdue_seconds = [deadline_overdue_seconds, handoff_overdue_seconds]
                .into_iter()
                .flatten()
                .max();
            let highest_risk_queue =
                highest_risk_queue_for_task(task, deadline_summary, queue_sets);

            TaskSlaSummary {
                task_id: task.task_id.clone(),
                due_soon_count,
                overdue_count,
                oldest_overdue_seconds,
                highest_risk_queue,
                breach_severity: classify_breach_severity(due_soon_count, overdue_count),
            }
        })
        .collect()
}

pub(super) fn highest_risk_queue_for_task(
    task: &Task,
    deadline_summary: Option<&TaskDeadlineSummary>,
    queue_sets: &SlaQueueSets<'_>,
) -> Option<SnapshotPreset> {
    let task_id = &task.task_id;
    [
        (
            queue_sets
                .overdue_review_decision_follow_through_task_ids
                .contains(task_id),
            SnapshotPreset::OverdueReviewDecisionFollowThrough,
        ),
        (
            queue_sets
                .overdue_review_handoff_follow_through_task_ids
                .contains(task_id),
            SnapshotPreset::OverdueReviewHandoffFollowThrough,
        ),
        (
            queue_sets
                .overdue_accepted_handoff_follow_through_task_ids
                .contains(task_id),
            SnapshotPreset::OverdueAcceptedHandoffFollowThrough,
        ),
        (
            queue_sets
                .overdue_handoff_acceptance_task_ids
                .contains(task_id),
            SnapshotPreset::OverdueHandoffAcceptance,
        ),
        (
            deadline_summary.is_some_and(|summary| summary.review_state == DeadlineState::Overdue),
            SnapshotPreset::OverdueReview,
        ),
        (
            deadline_summary.is_some_and(|summary| {
                summary.execution_state == DeadlineState::Overdue && task.owner_agent_id.is_some()
            }),
            SnapshotPreset::OverdueExecutionOwned,
        ),
        (
            deadline_summary.is_some_and(|summary| {
                summary.execution_state == DeadlineState::Overdue && task.owner_agent_id.is_none()
            }),
            SnapshotPreset::OverdueExecutionUnclaimed,
        ),
        (
            queue_sets
                .due_soon_review_decision_follow_through_task_ids
                .contains(task_id),
            SnapshotPreset::DueSoonReviewDecisionFollowThrough,
        ),
        (
            queue_sets
                .due_soon_review_handoff_follow_through_task_ids
                .contains(task_id),
            SnapshotPreset::DueSoonReviewHandoffFollowThrough,
        ),
        (
            queue_sets
                .due_soon_accepted_handoff_follow_through_task_ids
                .contains(task_id),
            SnapshotPreset::DueSoonAcceptedHandoffFollowThrough,
        ),
        (
            queue_sets
                .due_soon_handoff_acceptance_task_ids
                .contains(task_id),
            SnapshotPreset::DueSoonHandoffAcceptance,
        ),
        (
            deadline_summary.is_some_and(|summary| summary.review_state == DeadlineState::DueSoon),
            SnapshotPreset::DueSoonReview,
        ),
        (
            deadline_summary
                .is_some_and(|summary| summary.execution_state == DeadlineState::DueSoon),
            SnapshotPreset::DueSoonExecution,
        ),
    ]
    .into_iter()
    .find_map(|(active, preset)| active.then_some(preset))
}

pub(super) fn overdue_handoff_age_seconds(
    handoff: &Handoff,
    task: &Task,
    execution_summary: Option<&TaskExecutionSummary>,
    now: OffsetDateTime,
    overdue_queue_flags: OverdueTaskSlaQueues,
) -> Option<i64> {
    let due_at = parse_timestamp(handoff.due_at.as_deref()?)?;
    let is_follow_through = (overdue_queue_flags.review_handoff_follow_through
        && review_handoff_requires_follow_through(handoff, now))
        || (overdue_queue_flags.review_decision_follow_through
            && review_decision_requires_follow_through(handoff, now))
        || (overdue_queue_flags.accepted_handoff_follow_through
            && accepted_handoff_requires_follow_through(handoff, task, execution_summary));
    let is_acceptance = overdue_queue_flags.handoff_acceptance
        && handoff.status == crate::models::HandoffStatus::Open
        && !handoff_has_expired(handoff, now);

    ((is_follow_through || is_acceptance) && due_at < now).then_some((now - due_at).num_seconds())
}

pub(super) fn classify_breach_severity(
    due_soon_count: usize,
    overdue_count: usize,
) -> BreachSeverity {
    match (due_soon_count, overdue_count) {
        (0, 0) => BreachSeverity::None,
        (_, overdue_count) if overdue_count > 1 => BreachSeverity::Critical,
        (_, 1) => BreachSeverity::High,
        (due_soon_count, 0) if due_soon_count > 1 => BreachSeverity::Medium,
        _ => BreachSeverity::Low,
    }
}

pub(super) const fn breach_severity_rank(severity: BreachSeverity) -> usize {
    match severity {
        BreachSeverity::None => 0,
        BreachSeverity::Low => 1,
        BreachSeverity::Medium => 2,
        BreachSeverity::High => 3,
        BreachSeverity::Critical => 4,
    }
}

pub(super) fn summarize_sla(task_sla_summaries: &[TaskSlaSummary]) -> SnapshotSlaSummary {
    SnapshotSlaSummary {
        due_soon_count: task_sla_summaries
            .iter()
            .map(|summary| summary.due_soon_count)
            .sum(),
        overdue_count: task_sla_summaries
            .iter()
            .map(|summary| summary.overdue_count)
            .sum(),
        oldest_overdue_seconds: task_sla_summaries
            .iter()
            .filter(|summary| summary.overdue_count > 0)
            .filter_map(|summary| summary.oldest_overdue_seconds)
            .max(),
        breach_severity: task_sla_summaries
            .iter()
            .map(|summary| summary.breach_severity)
            .max_by_key(|severity| breach_severity_rank(*severity))
            .unwrap_or(BreachSeverity::None),
    }
}
