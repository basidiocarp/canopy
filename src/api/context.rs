#![allow(clippy::wildcard_imports)]

use super::*;

#[derive(Debug, Clone)]
pub(super) struct SnapshotContext {
    pub(super) now: OffsetDateTime,
    pub(super) open_handoff_task_ids: HashSet<String>,
    pub(super) pending_handoff_acceptance_task_ids: HashSet<String>,
    pub(super) due_soon_handoff_acceptance_task_ids: HashSet<String>,
    pub(super) overdue_handoff_acceptance_task_ids: HashSet<String>,
    pub(super) assigned_awaiting_claim_task_ids: HashSet<String>,
    pub(super) review_with_graph_pressure_task_ids: HashSet<String>,
    pub(super) review_handoff_follow_through_task_ids: HashSet<String>,
    pub(super) due_soon_review_handoff_follow_through_task_ids: HashSet<String>,
    pub(super) overdue_review_handoff_follow_through_task_ids: HashSet<String>,
    pub(super) due_soon_review_decision_follow_through_task_ids: HashSet<String>,
    pub(super) overdue_review_decision_follow_through_task_ids: HashSet<String>,
    pub(super) review_decision_follow_through_task_ids: HashSet<String>,
    pub(super) review_awaiting_support_task_ids: HashSet<String>,
    pub(super) review_ready_for_decision_task_ids: HashSet<String>,
    pub(super) review_ready_for_closeout_task_ids: HashSet<String>,
    pub(super) claimed_not_started_task_ids: HashSet<String>,
    pub(super) paused_resumable_task_ids: HashSet<String>,
    pub(super) accepted_handoff_follow_through_task_ids: HashSet<String>,
    pub(super) due_soon_accepted_handoff_follow_through_task_ids: HashSet<String>,
    pub(super) overdue_accepted_handoff_follow_through_task_ids: HashSet<String>,
    deadline_summary_by_task_id: HashMap<String, TaskDeadlineSummary>,
    relationship_summary_by_task_id: HashMap<String, TaskRelationshipSummary>,
}

impl SnapshotContext {
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(super) fn new(
        tasks: &[Task],
        handoffs: &[Handoff],
        task_events: &[TaskEvent],
        assignments: &[TaskAssignment],
        relationship_summaries: &[TaskRelationshipSummary],
        execution_summaries: &[TaskExecutionSummary],
        deadline_summaries: &[TaskDeadlineSummary],
        now: OffsetDateTime,
    ) -> Self {
        let accepted_handoff_follow_through_task_ids =
            derive_accepted_handoff_follow_through_task_ids(tasks, handoffs, execution_summaries);
        let due_soon_accepted_handoff_follow_through_task_ids =
            derive_accepted_handoff_follow_through_task_ids_with_freshness(
                tasks,
                handoffs,
                execution_summaries,
                now,
                Freshness::Aging,
            );
        let overdue_accepted_handoff_follow_through_task_ids =
            derive_accepted_handoff_follow_through_task_ids_with_freshness(
                tasks,
                handoffs,
                execution_summaries,
                now,
                Freshness::Stale,
            );
        let assigned_awaiting_claim_task_ids = derive_assigned_awaiting_claim_task_ids(
            tasks,
            assignments,
            execution_summaries,
            &accepted_handoff_follow_through_task_ids,
        );
        let review_with_graph_pressure_task_ids =
            derive_review_with_graph_pressure_task_ids(tasks, relationship_summaries);
        let review_handoff_follow_through_task_ids =
            derive_review_handoff_follow_through_task_ids(tasks, handoffs, now);
        let due_soon_review_handoff_follow_through_task_ids =
            derive_review_handoff_follow_through_task_ids_with_freshness(
                tasks,
                handoffs,
                now,
                Freshness::Aging,
            );
        let overdue_review_handoff_follow_through_task_ids =
            derive_review_handoff_follow_through_task_ids_with_freshness(
                tasks,
                handoffs,
                now,
                Freshness::Stale,
            );
        let due_soon_review_decision_follow_through_task_ids =
            derive_review_decision_follow_through_task_ids_with_freshness(
                tasks,
                handoffs,
                now,
                Freshness::Aging,
            );
        let overdue_review_decision_follow_through_task_ids =
            derive_review_decision_follow_through_task_ids_with_freshness(
                tasks,
                handoffs,
                now,
                Freshness::Stale,
            );
        let review_decision_follow_through_task_ids =
            derive_review_decision_follow_through_task_ids(tasks, handoffs, now);
        let review_awaiting_support_task_ids =
            derive_review_awaiting_support_task_ids(tasks, task_events);
        let review_ready_for_decision_task_ids = derive_review_ready_for_decision_task_ids(
            tasks,
            task_events,
            &review_with_graph_pressure_task_ids,
            &review_handoff_follow_through_task_ids,
            &review_decision_follow_through_task_ids,
            &review_awaiting_support_task_ids,
        );
        let review_ready_for_closeout_task_ids = derive_review_ready_for_closeout_task_ids(
            tasks,
            task_events,
            &review_with_graph_pressure_task_ids,
            &review_handoff_follow_through_task_ids,
            &review_decision_follow_through_task_ids,
            &review_awaiting_support_task_ids,
        );
        let claimed_not_started_task_ids = derive_claimed_not_started_task_ids(
            tasks,
            execution_summaries,
            &accepted_handoff_follow_through_task_ids,
        );
        let paused_resumable_task_ids = derive_paused_resumable_task_ids(
            tasks,
            execution_summaries,
            &accepted_handoff_follow_through_task_ids,
        );

        Self {
            now,
            open_handoff_task_ids: handoffs
                .iter()
                .filter(|handoff| handoff.status == crate::models::HandoffStatus::Open)
                .map(|handoff| handoff.task_id.clone())
                .collect(),
            pending_handoff_acceptance_task_ids: derive_pending_handoff_acceptance_task_ids(
                handoffs, now,
            ),
            due_soon_handoff_acceptance_task_ids:
                derive_pending_handoff_acceptance_task_ids_with_freshness(
                    handoffs,
                    now,
                    Freshness::Aging,
                ),
            overdue_handoff_acceptance_task_ids:
                derive_pending_handoff_acceptance_task_ids_with_freshness(
                    handoffs,
                    now,
                    Freshness::Stale,
                ),
            assigned_awaiting_claim_task_ids,
            review_with_graph_pressure_task_ids,
            review_handoff_follow_through_task_ids,
            due_soon_review_handoff_follow_through_task_ids,
            overdue_review_handoff_follow_through_task_ids,
            due_soon_review_decision_follow_through_task_ids,
            overdue_review_decision_follow_through_task_ids,
            review_decision_follow_through_task_ids,
            review_awaiting_support_task_ids,
            review_ready_for_decision_task_ids,
            review_ready_for_closeout_task_ids,
            claimed_not_started_task_ids,
            paused_resumable_task_ids,
            accepted_handoff_follow_through_task_ids,
            due_soon_accepted_handoff_follow_through_task_ids,
            overdue_accepted_handoff_follow_through_task_ids,
            deadline_summary_by_task_id: deadline_summaries
                .iter()
                .map(|summary| (summary.task_id.clone(), summary.clone()))
                .collect(),
            relationship_summary_by_task_id: relationship_summaries
                .iter()
                .map(|summary| (summary.task_id.clone(), summary.clone()))
                .collect(),
        }
    }

    pub(super) fn deadline_summary(&self, task_id: &str) -> Option<&TaskDeadlineSummary> {
        self.deadline_summary_by_task_id.get(task_id)
    }

    pub(super) fn relationship_summary(&self, task_id: &str) -> Option<&TaskRelationshipSummary> {
        self.relationship_summary_by_task_id.get(task_id)
    }

    pub(super) fn sla_queue_sets(&self) -> SlaQueueSets<'_> {
        SlaQueueSets {
            due_soon_handoff_acceptance_task_ids: &self.due_soon_handoff_acceptance_task_ids,
            overdue_handoff_acceptance_task_ids: &self.overdue_handoff_acceptance_task_ids,
            due_soon_accepted_handoff_follow_through_task_ids: &self
                .due_soon_accepted_handoff_follow_through_task_ids,
            overdue_accepted_handoff_follow_through_task_ids: &self
                .overdue_accepted_handoff_follow_through_task_ids,
            due_soon_review_handoff_follow_through_task_ids: &self
                .due_soon_review_handoff_follow_through_task_ids,
            overdue_review_handoff_follow_through_task_ids: &self
                .overdue_review_handoff_follow_through_task_ids,
            due_soon_review_decision_follow_through_task_ids: &self
                .due_soon_review_decision_follow_through_task_ids,
            overdue_review_decision_follow_through_task_ids: &self
                .overdue_review_decision_follow_through_task_ids,
        }
    }
}

pub(super) fn derive_pending_handoff_acceptance_task_ids(
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

pub(super) fn derive_pending_handoff_acceptance_task_ids_with_freshness(
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

pub(super) fn derive_accepted_handoff_follow_through_task_ids(
    tasks: &[Task],
    handoffs: &[Handoff],
    execution_summaries: &[TaskExecutionSummary],
) -> HashSet<String> {
    derive_accepted_handoff_follow_through_task_ids_inner(
        tasks,
        handoffs,
        execution_summaries,
        Utc::now(),
        None,
    )
}

pub(super) fn derive_accepted_handoff_follow_through_task_ids_with_freshness(
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
        now,
        Some(freshness),
    )
}

fn derive_accepted_handoff_follow_through_task_ids_inner(
    tasks: &[Task],
    handoffs: &[Handoff],
    execution_summaries: &[TaskExecutionSummary],
    now: OffsetDateTime,
    freshness: Option<Freshness>,
) -> HashSet<String> {
    let tasks_by_id: HashMap<_, _> = tasks
        .iter()
        .map(|task| (task.task_id.as_str(), task))
        .collect();
    let execution_summary_by_task_id: HashMap<_, _> = execution_summaries
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
            accepted_handoff_requires_follow_through(
                handoff,
                task,
                execution_summary_by_task_id
                    .get(handoff.task_id.as_str())
                    .copied(),
            )
            .then(|| handoff.task_id.clone())
        })
        .collect()
}

pub(super) fn accepted_handoff_requires_follow_through(
    handoff: &Handoff,
    task: &Task,
    execution_summary: Option<&TaskExecutionSummary>,
) -> bool {
    if handoff.status != crate::models::HandoffStatus::Accepted {
        return false;
    }
    if task.status != TaskStatus::Assigned {
        return false;
    }
    if task.owner_agent_id.as_deref() != Some(handoff.to_agent_id.as_str()) {
        return false;
    }
    let Some(resolved_at) = handoff.resolved_at.as_deref().and_then(parse_timestamp) else {
        return false;
    };
    let last_execution_after_acceptance = execution_summary
        .and_then(|summary| summary.last_execution_at.as_deref())
        .and_then(parse_timestamp)
        .is_some_and(|last_execution_at| last_execution_at >= resolved_at);

    !last_execution_after_acceptance
}

pub(super) fn derive_paused_resumable_task_ids(
    tasks: &[Task],
    execution_summaries: &[TaskExecutionSummary],
    accepted_handoff_follow_through_task_ids: &HashSet<String>,
) -> HashSet<String> {
    let execution_summary_by_task_id: HashMap<_, _> = execution_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();

    tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::Assigned
                && task.owner_agent_id.is_some()
                && !accepted_handoff_follow_through_task_ids.contains(&task.task_id)
                && execution_summary_by_task_id
                    .get(task.task_id.as_str())
                    .is_some_and(|summary| {
                        summary.run_count > 0
                            && summary.last_execution_action == Some(ExecutionActionKind::PauseTask)
                    })
        })
        .map(|task| task.task_id.clone())
        .collect()
}

pub(super) fn derive_claimed_not_started_task_ids(
    tasks: &[Task],
    execution_summaries: &[TaskExecutionSummary],
    accepted_handoff_follow_through_task_ids: &HashSet<String>,
) -> HashSet<String> {
    let execution_summary_by_task_id: HashMap<_, _> = execution_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();

    tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::Assigned
                && task.owner_agent_id.is_some()
                && !accepted_handoff_follow_through_task_ids.contains(&task.task_id)
                && execution_summary_by_task_id
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

pub(super) fn derive_assigned_awaiting_claim_task_ids(
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
    let execution_summary_by_task_id: HashMap<_, _> = execution_summaries
        .iter()
        .map(|summary| (summary.task_id.as_str(), summary))
        .collect();

    tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::Assigned
                && task.owner_agent_id.is_some()
                && assignments_by_task
                    .get(task.task_id.as_str())
                    .and_then(|history| history.last().copied())
                    .is_some_and(|last_assignment| {
                        if accepted_handoff_follow_through_task_ids.contains(&task.task_id) {
                            return false;
                        }
                        if Some(last_assignment.assigned_to.as_str())
                            != task.owner_agent_id.as_deref()
                        {
                            return false;
                        }
                        if last_assignment.assigned_by == last_assignment.assigned_to {
                            return false;
                        }
                        let last_execution_at = execution_summary_by_task_id
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

pub(super) fn derive_review_with_graph_pressure_task_ids(
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

pub(super) fn review_handoff_requires_follow_through(
    handoff: &Handoff,
    now: OffsetDateTime,
) -> bool {
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

pub(super) fn derive_review_handoff_follow_through_task_ids(
    tasks: &[Task],
    handoffs: &[Handoff],
    now: OffsetDateTime,
) -> HashSet<String> {
    derive_review_handoff_follow_through_task_ids_inner(tasks, handoffs, now, None)
}

pub(super) fn derive_review_handoff_follow_through_task_ids_with_freshness(
    tasks: &[Task],
    handoffs: &[Handoff],
    now: OffsetDateTime,
    freshness: Freshness,
) -> HashSet<String> {
    derive_review_handoff_follow_through_task_ids_inner(tasks, handoffs, now, Some(freshness))
}

fn derive_review_handoff_follow_through_task_ids_inner(
    tasks: &[Task],
    handoffs: &[Handoff],
    now: OffsetDateTime,
    freshness: Option<Freshness>,
) -> HashSet<String> {
    let handoff_task_ids: HashSet<_> = handoffs
        .iter()
        .filter(|handoff| review_handoff_requires_follow_through(handoff, now))
        .filter(|handoff| {
            freshness.is_none_or(|freshness| handoff_freshness(handoff, now) == freshness)
        })
        .map(|handoff| handoff.task_id.as_str())
        .collect();

    tasks
        .iter()
        .filter(|task| task.status == TaskStatus::ReviewRequired)
        .filter(|task| handoff_task_ids.contains(task.task_id.as_str()))
        .map(|task| task.task_id.clone())
        .collect()
}

pub(super) fn review_decision_requires_follow_through(
    handoff: &Handoff,
    now: OffsetDateTime,
) -> bool {
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

pub(super) fn derive_review_decision_follow_through_task_ids(
    tasks: &[Task],
    handoffs: &[Handoff],
    now: OffsetDateTime,
) -> HashSet<String> {
    derive_review_decision_follow_through_task_ids_inner(tasks, handoffs, now, None)
}

pub(super) fn derive_review_decision_follow_through_task_ids_with_freshness(
    tasks: &[Task],
    handoffs: &[Handoff],
    now: OffsetDateTime,
    freshness: Freshness,
) -> HashSet<String> {
    derive_review_decision_follow_through_task_ids_inner(tasks, handoffs, now, Some(freshness))
}

fn derive_review_decision_follow_through_task_ids_inner(
    tasks: &[Task],
    handoffs: &[Handoff],
    now: OffsetDateTime,
    freshness: Option<Freshness>,
) -> HashSet<String> {
    let handoff_task_ids: HashSet<_> = handoffs
        .iter()
        .filter(|handoff| review_decision_requires_follow_through(handoff, now))
        .filter(|handoff| {
            freshness.is_none_or(|freshness| handoff_freshness(handoff, now) == freshness)
        })
        .map(|handoff| handoff.task_id.as_str())
        .collect();

    tasks
        .iter()
        .filter(|task| task.status == TaskStatus::ReviewRequired)
        .filter(|task| handoff_task_ids.contains(task.task_id.as_str()))
        .map(|task| task.task_id.clone())
        .collect()
}

pub(super) fn derive_review_awaiting_support_task_ids(
    tasks: &[Task],
    events: &[TaskEvent],
) -> HashSet<String> {
    let mut events_by_task_id: HashMap<&str, Vec<&TaskEvent>> = HashMap::new();
    for event in events {
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

pub(super) fn derive_review_ready_for_closeout_task_ids(
    tasks: &[Task],
    events: &[TaskEvent],
    review_with_graph_pressure_task_ids: &HashSet<String>,
    review_handoff_follow_through_task_ids: &HashSet<String>,
    review_decision_follow_through_task_ids: &HashSet<String>,
    review_awaiting_support_task_ids: &HashSet<String>,
) -> HashSet<String> {
    let mut events_by_task_id: HashMap<&str, Vec<&TaskEvent>> = HashMap::new();
    for event in events {
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

pub(super) fn derive_review_ready_for_decision_task_ids(
    tasks: &[Task],
    events: &[TaskEvent],
    review_with_graph_pressure_task_ids: &HashSet<String>,
    review_handoff_follow_through_task_ids: &HashSet<String>,
    review_decision_follow_through_task_ids: &HashSet<String>,
    review_awaiting_support_task_ids: &HashSet<String>,
) -> HashSet<String> {
    let mut events_by_task_id: HashMap<&str, Vec<&TaskEvent>> = HashMap::new();
    for event in events {
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

pub(super) fn heartbeat_matches_tasks(
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

pub(super) fn handoff_freshness(handoff: &Handoff, now: OffsetDateTime) -> Freshness {
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
        let minutes_until_due = (due_at - now).num_minutes();
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

pub(super) fn timestamp_freshness(
    timestamp: &str,
    now: OffsetDateTime,
    aging_hours: i64,
    stale_hours: i64,
) -> Freshness {
    let Some(parsed) = parse_timestamp(timestamp) else {
        return Freshness::Missing;
    };
    let elapsed_hours = (now - parsed).num_hours();
    if elapsed_hours >= stale_hours {
        Freshness::Stale
    } else if elapsed_hours >= aging_hours {
        Freshness::Aging
    } else {
        Freshness::Fresh
    }
}

pub(super) fn heartbeat_freshness(timestamp: Option<&str>, now: OffsetDateTime) -> Freshness {
    let Some(timestamp) = timestamp else {
        return Freshness::Missing;
    };
    let Some(parsed) = parse_timestamp(timestamp) else {
        return Freshness::Missing;
    };
    let elapsed_minutes = (now - parsed).num_minutes();
    if elapsed_minutes >= HEARTBEAT_STALE_MINUTES {
        Freshness::Stale
    } else if elapsed_minutes >= HEARTBEAT_AGING_MINUTES {
        Freshness::Aging
    } else {
        Freshness::Fresh
    }
}

pub(super) fn parse_timestamp(raw: &str) -> Option<OffsetDateTime> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
                .map(|dt| dt.and_utc())
                .ok()
        })
}

pub(super) fn max_freshness(left: Freshness, right: Freshness) -> Freshness {
    if freshness_rank(left) >= freshness_rank(right) {
        left
    } else {
        right
    }
}

pub(super) fn compare_timestamp_desc(left: &str, right: &str) -> std::cmp::Ordering {
    match (parse_timestamp(left), parse_timestamp(right)) {
        (Some(left), Some(right)) => right.cmp(&left),
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

pub(super) fn freshness_sort_rank(freshness: Freshness) -> u8 {
    match freshness {
        Freshness::Missing => 0,
        Freshness::Stale => 1,
        Freshness::Aging => 2,
        Freshness::Fresh => 3,
    }
}
