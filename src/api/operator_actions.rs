#![allow(clippy::wildcard_imports)]

use super::*;

#[allow(dead_code)]
#[allow(clippy::too_many_lines)]
pub(super) fn derive_operator_actions(
    tasks: &[Task],
    task_attention: &[TaskAttention],
    deadline_summaries: &[TaskDeadlineSummary],
    relationship_summaries: &[TaskRelationshipSummary],
    execution_summaries: &[TaskExecutionSummary],
    handoffs: &[Handoff],
    handoff_attention: &[HandoffAttention],
    workflow_contexts: &[TaskWorkflowContext],
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
    let workflow_by_task_id: HashMap<_, _> = workflow_contexts
        .iter()
        .map(|context| (context.task_id.as_str(), context))
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
        let workflow_context = workflow_by_task_id.get(task.task_id.as_str()).copied();

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

        if task.status == TaskStatus::ReviewRequired
            && workflow_context.is_some_and(|context| {
                context.council_session_id.is_none()
                    && context.review_cycle.as_ref().is_some_and(|cycle| {
                        matches!(
                            cycle.state,
                            ReviewCycleState::Pending | ReviewCycleState::InReview
                        )
                    })
            })
        {
            actions.push(OperatorAction {
                action_id: format!("task:{}:summon_council", task.task_id),
                kind: OperatorActionKind::SummonCouncilSession,
                target_kind: OperatorActionTargetKind::Task,
                level: if attention.level == AttentionLevel::Normal {
                    AttentionLevel::NeedsAttention
                } else {
                    attention.level
                },
                task_id: Some(task.task_id.clone()),
                handoff_id: None,
                agent_id: task.owner_agent_id.clone(),
                title: format!("Summon council for {}", task.title),
                summary: "Active review cycle has not been linked to a council session yet."
                    .to_string(),
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

#[allow(clippy::too_many_lines)]
pub(super) fn derive_allowed_handoff_actions(
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

pub(super) fn make_task_allowed_action(
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

pub(super) fn handoff_has_expired(handoff: &Handoff, now: OffsetDateTime) -> bool {
    handoff
        .expires_at
        .as_deref()
        .and_then(parse_timestamp)
        .is_some_and(|expires_at| expires_at <= now)
}
