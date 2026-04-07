use super::operator_actions::derive_allowed_handoff_actions;
use super::*;

#[allow(dead_code)]
pub(super) fn derive_allowed_actions(
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

pub(super) fn derive_allowed_task_actions(
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
