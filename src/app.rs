mod commands;
mod server;

use crate::db;
use anyhow::{Context, Result};
use canopy::api;
use canopy::cli::{
    AgentCommand, ApiCommand, Cli, Commands, CouncilCommand, EvidenceCommand, FilesCommand,
    HandoffCommand, NotificationCommand, OutcomeCommand, PolicyCommand, TaskCommand,
};
use canopy::models::{
    AgentRegistration, AgentRole, AgentStatus, CouncilSession, EvidenceRef, EvidenceSourceKind,
    EvidenceVerificationReport, EvidenceVerificationResult, EvidenceVerificationStatus, Task,
    TaskAction, TaskRelationshipKind, TaskStatus, VerificationState,
};
use canopy::runtime::{DispatchDecision, pre_dispatch_check};
use canopy::store::{
    CLAIM_STALE_THRESHOLD_SECS, EvidenceLinkRefs, HandoffOperatorActionInput, HandoffTiming, Store,
    TaskCreationOptions, TaskStatusUpdate, TaskTriageUpdate, agent_last_heartbeat_age_secs,
    classify_agent_freshness,
};
use canopy::tools::evidence::build_evidence_review_rows;
use clap::Parser;
use serde::Serialize;
use serde_json::Value;
use spore::logging::{
    LogOutput, LoggingConfig, SpanContext, SpanEvents, root_span, subprocess_span, tool_span,
    workflow_span,
};
use spore::{Tool, discover};
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::Level;
use tracing::warn;

const EVIDENCE_VERIFY_SCHEMA_VERSION: &str = "1.0";

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    spore::logging::init_with_config(
        LoggingConfig::for_app("canopy", Level::WARN)
            .with_output(LogOutput::Stderr)
            .with_span_events(SpanEvents::Lifecycle),
    );
    let _telemetry = spore::telemetry::init_tracer("canopy").unwrap_or_else(|e| {
        tracing::debug!("OTel init skipped: {}", e);
        spore::telemetry::TelemetryInit::disabled("canopy")
    });
    let span_context = command_span_context(&cli);
    let _root_span = root_span(&span_context).entered();
    let _workflow_span = workflow_span(command_name(&cli.command), &span_context).entered();
    let store = db::open(cli.db.as_deref())?;
    commands::run(&store, cli.command)
}

fn handle_agent_command(store: &Store, command: AgentCommand) -> Result<()> {
    match command {
        AgentCommand::Register {
            agent_id,
            host_id,
            host_type,
            host_instance,
            model,
            project_root,
            worktree_id,
            role,
            capabilities,
        } => {
            let agent = AgentRegistration {
                agent_id,
                host_id,
                host_type,
                host_instance,
                model,
                project_root,
                worktree_id,
                role,
                capabilities,
                status: AgentStatus::Idle,
                current_task_id: None,
                heartbeat_at: None,
            };
            print_json(&store.register_agent(&agent)?)?;
        }
        AgentCommand::Heartbeat {
            agent_id,
            status,
            current_task_id,
        } => {
            let agent = store.heartbeat_agent(&agent_id, status, current_task_id.as_deref())?;
            print_json(&agent)?;
        }
        AgentCommand::List => {
            let agents = store.list_agents()?;
            let mut output = Vec::with_capacity(agents.len());
            for agent in agents {
                let freshness = classify_agent_freshness(agent_last_heartbeat_age_secs(
                    store,
                    &agent.agent_id,
                )?);
                let mut value = serde_json::to_value(&agent)?;
                if let Value::Object(ref mut object) = value {
                    object.insert("freshness".to_string(), serde_json::to_value(freshness)?);
                }
                output.push(value);
            }
            print_json(&output)?;
        }
        AgentCommand::History {
            agent_id,
            task_id,
            limit,
        } => {
            print_json(&store.list_agent_heartbeats(
                agent_id.as_deref(),
                task_id.as_deref(),
                limit,
            )?)?;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_lines)]
fn handle_task_command(store: &Store, command: TaskCommand) -> Result<()> {
    match command {
        TaskCommand::Create {
            title,
            description,
            requested_by,
            project_root,
            parent,
            required_role,
            required_capabilities,
            auto_review,
            verification_required,
            scope,
            workflow_id,
            phase_id,
        } => {
            let options = TaskCreationOptions {
                required_role,
                required_capabilities,
                auto_review,
                verification_required,
                scope,
                workflow_id,
                phase_id,
            };
            let task = if let Some(parent_task_id) = parent.as_deref() {
                store.create_subtask_with_options(
                    parent_task_id,
                    &title,
                    description.as_deref(),
                    &requested_by,
                    &options,
                )?
            } else {
                store.create_task_with_options(
                    &title,
                    description.as_deref(),
                    &requested_by,
                    &project_root,
                    &options,
                )?
            };
            print_json(&task)?;
        }
        TaskCommand::Assign {
            task_id,
            assigned_to,
            assigned_by,
            reason,
        } => {
            canopy::store::ensure_capabilities_match(store, &task_id, &assigned_to)?;
            let task =
                store.assign_task(&task_id, &assigned_to, &assigned_by, reason.as_deref())?;
            print_json(&task)?;
        }
        TaskCommand::Status {
            task_id,
            status,
            changed_by,
            verification_state,
            blocked_reason,
            closure_summary,
        } => {
            let task = store.update_task_status(
                &task_id,
                status,
                &changed_by,
                TaskStatusUpdate {
                    verification_state,
                    blocked_reason: blocked_reason.as_deref(),
                    closure_summary: closure_summary.as_deref(),
                    event_note: None,
                },
            )?;
            print_json(&task)?;
        }
        TaskCommand::Triage {
            task_id,
            changed_by,
            priority,
            severity,
            acknowledged,
            owner_note,
            clear_owner_note,
        } => {
            let task = store.update_task_triage(
                &task_id,
                &changed_by,
                TaskTriageUpdate {
                    priority,
                    severity,
                    acknowledged,
                    owner_note: owner_note.as_deref(),
                    clear_owner_note,
                    event_note: None,
                },
            )?;
            print_json(&task)?;
        }
        TaskCommand::Action {
            task_id,
            action,
            changed_by,
            acting_agent_id,
            assigned_to,
            priority,
            severity,
            verification_state,
            blocked_reason,
            closure_summary,
            owner_note,
            clear_owner_note,
            note,
            from_agent_id,
            to_agent_id,
            handoff_type,
            handoff_summary,
            requested_action,
            due_at,
            review_due_at,
            expires_at,
            author_agent_id,
            message_type,
            message_body,
            evidence_source_kind,
            evidence_source_ref,
            evidence_label,
            evidence_summary,
            related_handoff_id,
            related_session_id,
            related_memory_query,
            related_symbol,
            related_file,
            follow_up_title,
            follow_up_description,
            related_task_id,
            relationship_role,
        } => {
            let fallback_session_id = runtime_session_id_from_env();
            let resolved_session_id = related_session_id
                .as_deref()
                .or(fallback_session_id.as_deref());
            let task_action = cli_action_to_task_action(
                action,
                note.as_deref(),
                acting_agent_id.as_deref(),
                assigned_to.as_deref(),
                priority,
                severity,
                verification_state,
                blocked_reason.as_deref(),
                closure_summary.as_deref(),
                owner_note.as_deref(),
                clear_owner_note,
                from_agent_id.as_deref(),
                to_agent_id.as_deref(),
                handoff_type,
                handoff_summary.as_deref(),
                requested_action.as_deref(),
                due_at.as_deref(),
                review_due_at.as_deref(),
                expires_at.as_deref(),
                author_agent_id.as_deref(),
                message_type,
                message_body.as_deref(),
                evidence_source_kind,
                evidence_source_ref.as_deref(),
                evidence_label.as_deref(),
                evidence_summary.as_deref(),
                related_handoff_id.as_deref(),
                resolved_session_id,
                related_memory_query.as_deref(),
                related_symbol.as_deref(),
                related_file.as_deref(),
                follow_up_title.as_deref(),
                follow_up_description.as_deref(),
                related_task_id.as_deref(),
                relationship_role,
            )?;
            let task = store.apply_task_operator_action(&task_id, &changed_by, task_action)?;
            print_json(&task)?;
        }
        TaskCommand::Verify {
            task_id,
            script,
            step,
        } => {
            print_json(&run_task_verification(
                store,
                &task_id,
                &script,
                step.as_deref(),
            )?)?;
        }
        TaskCommand::List { tree } => {
            if tree {
                let tasks = store.list_tasks()?;
                let relationships = store.list_task_relationships(None)?;

                // Build parent->child map
                let mut children_map: std::collections::HashMap<String, Vec<String>> =
                    std::collections::HashMap::new();
                for rel in &relationships {
                    if rel.kind == TaskRelationshipKind::Parent {
                        children_map
                            .entry(rel.target_task_id.clone())
                            .or_default()
                            .push(rel.source_task_id.clone());
                    }
                }

                // Find root tasks (tasks with no parent)
                let mut root_ids = Vec::new();
                for task in &tasks {
                    if task.parent_task_id.is_none() {
                        root_ids.push(task.task_id.clone());
                    }
                }
                root_ids.sort();

                // Render tree
                let mut output = String::from("TASK LIST\n");
                for (idx, root_id) in root_ids.iter().enumerate() {
                    let is_last = idx == root_ids.len() - 1;
                    if let Some(root_task) = tasks.iter().find(|t| t.task_id == *root_id) {
                        render_task_tree(
                            &mut output,
                            root_task,
                            &tasks,
                            &children_map,
                            "",
                            is_last,
                        );
                    }
                }
                println!("{output}");
            } else {
                print_json(&store.list_tasks()?)?;
            }
        }
        TaskCommand::ListView {
            project_root,
            preset,
            view,
            sort,
            priority_at_least,
            severity_at_least,
            acknowledged,
            attention_at_least,
        } => {
            let snapshot = api::snapshot(
                store,
                api::SnapshotOptions {
                    project_root: project_root.as_deref(),
                    preset,
                    sort,
                    view,
                    priority_at_least,
                    severity_at_least,
                    acknowledged,
                    attention_at_least,
                },
            )?;
            print_json(&snapshot.tasks)?;
        }
        TaskCommand::Show { task_id } => {
            print_json(&store.get_task(&task_id)?)?;
        }
        TaskCommand::Claim {
            agent_id,
            task_id,
            force_claim,
            after,
            worktree,
        } => {
            if !force_claim {
                canopy::store::ensure_agent_fresh_for_claim(
                    store,
                    &agent_id,
                    CLAIM_STALE_THRESHOLD_SECS,
                )?;
            }

            // Capability check before any mutations.
            canopy::store::ensure_capabilities_match(store, &task_id, &agent_id)?;

            // Sequential mode: add BlockedBy relationship before claiming
            if let Some(ref blocker_id) = after {
                store.add_task_relationship(
                    &task_id,
                    blocker_id,
                    TaskRelationshipKind::Blocks,
                    &agent_id,
                )?;
                warn!(
                    task_id = %task_id,
                    blocker_id = %blocker_id,
                    "added dependency before claim"
                );
            }

            // Worktree isolation: record worktree ID in task metadata note
            if worktree {
                let worktree_id = format!("canopy-{}", &task_id[..task_id.len().min(8)]);
                store.update_task_status(
                    &task_id,
                    TaskStatus::Open,
                    &agent_id,
                    TaskStatusUpdate {
                        event_note: Some(&format!("worktree_id={worktree_id}")),
                        ..Default::default()
                    },
                )?;
                warn!(
                    task_id = %task_id,
                    worktree_id = %worktree_id,
                    "recorded worktree isolation for claim"
                );
            }

            // Scope conflict check before claiming
            if !force_claim {
                let task = store.get_task(&task_id)?;
                if !task.scope.is_empty() {
                    let conflicts = store.find_scope_conflicts(&task_id, &task.scope)?;
                    if !conflicts.is_empty() {
                        let detail = conflicts
                            .iter()
                            .map(|c| {
                                format!(
                                    "  {} (task {}, agent {}): {}",
                                    c.task_title,
                                    c.task_id,
                                    c.agent_id,
                                    c.overlapping_paths.join(", ")
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        anyhow::bail!(
                            "File scope conflict detected:\n{detail}\n\
                             Resolve with: --after <task-id> (sequential), \
                             --worktree (isolated), or --force-claim (advisory)"
                        );
                    }
                }
            }

            let claimed_task = store
                .atomic_claim_task(&agent_id, &task_id)?
                .ok_or_else(|| anyhow::anyhow!("task already claimed or not found"))?;
            print_json(&claimed_task)?;
        }
        TaskCommand::Complete {
            agent_id,
            task_id,
            summary,
            force,
        } => {
            // Gate: if verification_required=true, check for passing ScriptVerification evidence
            let task_record = store.get_task(&task_id)?;

            if task_record.verification_required && !force {
                let evidence = store.list_evidence(&task_id).unwrap_or_default();
                let has_passing_verification = evidence.iter().any(|e| {
                    matches!(e.source_kind, EvidenceSourceKind::ScriptVerification)
                        && e.summary
                            .as_deref()
                            .is_some_and(|s| s.contains("script verification passed"))
                });
                if !has_passing_verification {
                    return Err(anyhow::anyhow!(
                        "task {task_id} requires script verification before completion.\n\n\
                         Attach a passing verification result:\n  \
                         canopy evidence add --task-id {task_id} --source-kind script_verification \\\n    \
                         --source-ref <ref> --label verification --summary 'script verification passed'\n\n\
                         Or override (operators only):\n  \
                         canopy task complete {task_id} --agent-id {agent_id} --summary '{summary}' --force"
                    ));
                }
            }

            // Gate: check for open child tasks (unless --force is used)
            if !force {
                let open_children = store.list_open_child_tasks(&task_id)?;
                if !open_children.is_empty() {
                    let mut child_list = String::new();
                    for (child_id, child_title, child_status) in &open_children {
                        let _ =
                            writeln!(child_list, "  {child_id}  {child_title}  [{child_status}]");
                    }
                    return Err(anyhow::anyhow!(
                        "task {task_id} has {} open sub-task(s).\n\n\
                         Complete or cancel all sub-tasks first, or use --force to override.\n\n\
                         Open sub-tasks:\n{child_list}\n\
                         To override:\n  \
                         canopy task complete {task_id} --agent-id {agent_id} --summary '{summary}' --force",
                        open_children.len()
                    ));
                }
            }

            let task = store.update_task_status(
                &task_id,
                TaskStatus::Completed,
                &agent_id,
                TaskStatusUpdate {
                    verification_state: None,
                    blocked_reason: None,
                    closure_summary: Some(&summary),
                    event_note: None,
                },
            )?;
            store.add_evidence(
                &task_id,
                EvidenceSourceKind::ManualNote,
                &task_id,
                "completion_summary",
                Some(&summary),
                EvidenceLinkRefs::default(),
            )?;

            // Log force override if applicable
            if force && task_record.verification_required {
                store.add_evidence(
                    &task_id,
                    EvidenceSourceKind::ManualNote,
                    &task_id,
                    "verification_override",
                    Some("completion allowed with --force override despite missing verification"),
                    EvidenceLinkRefs::default(),
                )?;
            }

            if force
                && !store
                    .list_open_child_tasks(&task_id)
                    .unwrap_or_default()
                    .is_empty()
            {
                store.add_evidence(
                    &task_id,
                    EvidenceSourceKind::ManualNote,
                    &task_id,
                    "children_override",
                    Some("completion allowed with --force override despite open sub-tasks"),
                    EvidenceLinkRefs::default(),
                )?;
            }

            print_json(&task)?;
        }
    }

    Ok(())
}

fn render_task_tree(
    output: &mut String,
    task: &Task,
    all_tasks: &[Task],
    children_map: &std::collections::HashMap<String, Vec<String>>,
    prefix: &str,
    is_last: bool,
) {
    // Add current task
    let current_prefix = if is_last { "└─ " } else { "├─ " };
    let _ = writeln!(
        output,
        "{prefix}{current_prefix}{} [{}]      {}",
        task.task_id, task.status, task.title
    );

    // Add children
    if let Some(child_ids) = children_map.get(&task.task_id) {
        let next_prefix = if is_last { "   " } else { "│  " };
        for (idx, child_id) in child_ids.iter().enumerate() {
            if let Some(child_task) = all_tasks.iter().find(|t| &t.task_id == child_id) {
                let is_last_child = idx == child_ids.len() - 1;
                render_task_tree(
                    output,
                    child_task,
                    all_tasks,
                    children_map,
                    &format!("{prefix}{next_prefix}"),
                    is_last_child,
                );
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct ImportedHandoffStep {
    task_id: String,
    title: String,
}

#[derive(Debug, Serialize)]
struct ImportedHandoff {
    path: String,
    verify_script: Option<String>,
    requested_assignee: Option<String>,
    assigned_to: Option<String>,
    review_hold_reason: Option<String>,
    parent_task: Task,
    steps: Vec<ImportedHandoffStep>,
}

#[derive(Debug)]
struct ParsedHandoffStep {
    description: Option<String>,
    step_marker: String,
    scope: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TaskVerificationRun {
    task: Task,
    passed: bool,
    script: String,
    step: Option<String>,
    output: String,
}

/// Converts CLI `--action` flag and optional fields into a typed `TaskAction`.
///
/// The CLI `task action` command uses a single `OperatorActionKind` flag with 33 optional
/// fields for backward compatibility. This function bridges from that flat representation
/// to the typed enum at the CLI boundary.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn cli_action_to_task_action<'a>(
    action: canopy::models::OperatorActionKind,
    note: Option<&'a str>,
    acting_agent_id: Option<&'a str>,
    assigned_to: Option<&'a str>,
    priority: Option<canopy::models::TaskPriority>,
    severity: Option<canopy::models::TaskSeverity>,
    verification_state: Option<canopy::models::VerificationState>,
    blocked_reason: Option<&'a str>,
    closure_summary: Option<&'a str>,
    owner_note: Option<&'a str>,
    clear_owner_note: bool,
    from_agent_id: Option<&'a str>,
    to_agent_id: Option<&'a str>,
    handoff_type: Option<canopy::models::HandoffType>,
    handoff_summary: Option<&'a str>,
    requested_action: Option<&'a str>,
    due_at: Option<&'a str>,
    review_due_at: Option<&'a str>,
    expires_at: Option<&'a str>,
    author_agent_id: Option<&'a str>,
    message_type: Option<canopy::models::CouncilMessageType>,
    message_body: Option<&'a str>,
    evidence_source_kind: Option<canopy::models::EvidenceSourceKind>,
    evidence_source_ref: Option<&'a str>,
    evidence_label: Option<&'a str>,
    evidence_summary: Option<&'a str>,
    related_handoff_id: Option<&'a str>,
    related_session_id: Option<&'a str>,
    related_memory_query: Option<&'a str>,
    related_symbol: Option<&'a str>,
    related_file: Option<&'a str>,
    follow_up_title: Option<&'a str>,
    follow_up_description: Option<&'a str>,
    related_task_id: Option<&'a str>,
    relationship_role: Option<canopy::models::TaskRelationshipRole>,
) -> Result<TaskAction<'a>> {
    use canopy::models::OperatorActionKind as K;
    let require = |opt: Option<&'a str>, field: &str| -> Result<&'a str> {
        opt.ok_or_else(|| anyhow::anyhow!("{action} requires --{field}"))
    };
    let task_action = match action {
        K::AcknowledgeTask => TaskAction::Acknowledge { note },
        K::UnacknowledgeTask => TaskAction::Unacknowledge { note },
        K::SetTaskPriority => TaskAction::SetPriority {
            priority: priority.ok_or_else(|| anyhow::anyhow!("{action} requires --priority"))?,
            note,
        },
        K::SetTaskSeverity => TaskAction::SetSeverity {
            severity: severity.ok_or_else(|| anyhow::anyhow!("{action} requires --severity"))?,
            note,
        },
        K::UpdateTaskNote => TaskAction::UpdateNote {
            owner_note,
            clear_owner_note,
            note,
        },
        K::SetTaskDueAt => TaskAction::SetDueAt {
            due_at: require(due_at, "due-at")?,
            note,
        },
        K::ClearTaskDueAt => TaskAction::ClearDueAt { note },
        K::SetReviewDueAt => TaskAction::SetReviewDueAt {
            review_due_at: require(review_due_at, "review-due-at")?,
            note,
        },
        K::ClearReviewDueAt => TaskAction::ClearReviewDueAt { note },
        K::VerifyTask => TaskAction::Verify {
            verification_state: verification_state
                .ok_or_else(|| anyhow::anyhow!("{action} requires --verification-state"))?,
            note,
        },
        K::CloseTask => TaskAction::Close {
            closure_summary: require(closure_summary, "closure-summary")?,
            note,
        },
        K::BlockTask => TaskAction::Block {
            blocked_reason: require(blocked_reason, "blocked-reason")?,
            note,
        },
        K::UnblockTask => TaskAction::Unblock { note },
        K::ReopenBlockedTaskWhenUnblocked => TaskAction::ReopenWhenUnblocked { note },
        K::ClaimTask => TaskAction::Claim {
            acting_agent_id: require(acting_agent_id, "acting-agent-id")?,
            note,
        },
        K::StartTask => TaskAction::Start {
            acting_agent_id: require(acting_agent_id, "acting-agent-id")?,
            note,
        },
        K::ResumeTask => TaskAction::Resume {
            acting_agent_id: require(acting_agent_id, "acting-agent-id")?,
            note,
        },
        K::PauseTask => TaskAction::Pause {
            acting_agent_id: require(acting_agent_id, "acting-agent-id")?,
            note,
        },
        K::YieldTask => TaskAction::Yield {
            acting_agent_id: require(acting_agent_id, "acting-agent-id")?,
            note,
        },
        K::CompleteTask => TaskAction::Complete {
            acting_agent_id: require(acting_agent_id, "acting-agent-id")?,
            note,
        },
        K::ReassignTask => TaskAction::Reassign {
            assigned_to: require(assigned_to, "assigned-to")?,
            note,
        },
        K::RecordDecision => TaskAction::RecordDecision {
            author_agent_id: require(author_agent_id, "author-agent-id")?,
            message_body: require(message_body, "message-body")?,
        },
        K::CreateHandoff => TaskAction::CreateHandoff {
            from_agent_id: require(from_agent_id, "from-agent-id")?,
            to_agent_id: require(to_agent_id, "to-agent-id")?,
            handoff_type: handoff_type
                .ok_or_else(|| anyhow::anyhow!("{action} requires --handoff-type"))?,
            handoff_summary: require(handoff_summary, "handoff-summary")?,
            requested_action,
            due_at,
            expires_at,
        },
        K::SummonCouncilSession => TaskAction::SummonCouncilSession,
        K::PostCouncilMessage => TaskAction::PostCouncilMessage {
            author_agent_id: require(author_agent_id, "author-agent-id")?,
            message_type: message_type
                .ok_or_else(|| anyhow::anyhow!("{action} requires --message-type"))?,
            message_body: require(message_body, "message-body")?,
        },
        K::AttachEvidence => TaskAction::AttachEvidence {
            source_kind: evidence_source_kind
                .ok_or_else(|| anyhow::anyhow!("{action} requires --evidence-source-kind"))?,
            source_ref: require(evidence_source_ref, "evidence-source-ref")?,
            label: require(evidence_label, "evidence-label")?,
            summary: evidence_summary,
            related_handoff_id,
            related_session_id,
            related_memory_query,
            related_symbol,
            related_file,
        },
        K::CreateFollowUpTask => TaskAction::CreateFollowUp {
            title: require(follow_up_title, "follow-up-title")?,
            description: follow_up_description,
        },
        K::LinkTaskDependency => TaskAction::LinkDependency {
            related_task_id: require(related_task_id, "related-task-id")?,
            relationship_role: relationship_role
                .ok_or_else(|| anyhow::anyhow!("{action} requires --relationship-role"))?,
        },
        K::ResolveDependency => TaskAction::ResolveDependency {
            related_task_id: require(related_task_id, "related-task-id")?,
        },
        K::PromoteFollowUp => TaskAction::PromoteFollowUp {
            related_task_id: require(related_task_id, "related-task-id")?,
        },
        K::CloseFollowUpChain => TaskAction::CloseFollowUpChain,
        K::AcceptHandoff
        | K::RejectHandoff
        | K::CancelHandoff
        | K::CompleteHandoff
        | K::FollowUpHandoff
        | K::ExpireHandoff => {
            anyhow::bail!("operator action {action} is not valid for tasks")
        }
    };
    Ok(task_action)
}

#[allow(clippy::too_many_lines)]
fn handle_import_handoff(store: &Store, path: &Path, assign: Option<&str>) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("read handoff markdown {}", path.display()))?;
    let title = extract_handoff_title(&content);
    let steps = extract_handoff_steps(&content);
    let verify_script = verification_script_path(path);
    let verify_script_exists = verify_script.exists();
    let project_root = infer_handoff_project_root(path)?;
    let parent_description = Some(format!("Imported from {}", path.display()));
    let parent_task = store.create_task_with_options(
        &title,
        parent_description.as_deref(),
        "handoff-import",
        &project_root,
        &TaskCreationOptions {
            required_role: Some(AgentRole::Implementer),
            verification_required: true,
            ..TaskCreationOptions::default()
        },
    )?;

    if verify_script_exists {
        store.add_evidence(
            &parent_task.task_id,
            EvidenceSourceKind::ManualNote,
            &path.display().to_string(),
            "Verification command",
            Some(&format!(
                "Run: canopy task verify --task-id {} --script {}",
                parent_task.task_id,
                verify_script.display()
            )),
            EvidenceLinkRefs::default(),
        )?;
    }

    let mut imported_steps = Vec::with_capacity(steps.len());
    for step in steps {
        let task = store.create_subtask_with_options(
            &parent_task.task_id,
            &step.step_marker,
            step.description.as_deref(),
            "handoff-import",
            &TaskCreationOptions {
                required_role: Some(AgentRole::Implementer),
                verification_required: true,
                scope: step.scope,
                ..TaskCreationOptions::default()
            },
        )?;
        if verify_script_exists {
            store.add_evidence(
                &task.task_id,
                EvidenceSourceKind::ManualNote,
                &path.display().to_string(),
                "Verification command",
                Some(&format!(
                    "Run: canopy task verify --task-id {} --script {} --step '{}'",
                    task.task_id,
                    verify_script.display(),
                    step.step_marker
                )),
                EvidenceLinkRefs::default(),
            )?;
        }
        imported_steps.push(ImportedHandoffStep {
            task_id: task.task_id,
            title: task.title,
        });
    }

    let mut assigned_to = None;
    let mut review_hold_reason = None;
    let parent_task = if let Some(agent_id) = assign {
        match pre_dispatch_check(path) {
            DispatchDecision::Proceed => {
                assigned_to = Some(agent_id.to_string());
                store.assign_task(
                    &parent_task.task_id,
                    agent_id,
                    "handoff-import",
                    Some("assigned during handoff import"),
                )?
            }
            DispatchDecision::FlagForReview { reason } => {
                warn!(
                    path = %path.display(),
                    reason = %reason,
                    "holding handoff for human review"
                );
                review_hold_reason = Some(reason);
                parent_task
            }
        }
    } else {
        parent_task
    };

    print_json(&ImportedHandoff {
        path: path.display().to_string(),
        verify_script: verify_script_exists.then(|| verify_script.display().to_string()),
        requested_assignee: assign.map(ToOwned::to_owned),
        assigned_to,
        review_hold_reason,
        parent_task,
        steps: imported_steps,
    })
}

fn run_task_verification(
    store: &Store,
    task_id: &str,
    script: &Path,
    step: Option<&str>,
) -> Result<TaskVerificationRun> {
    let script_display = script.display().to_string();
    let output = verification_script_command(script)
        .output()
        .with_context(|| format!("run verification script {script_display}"))?;
    let combined = combine_command_output(&output);
    let filtered = filter_verification_output(&combined, step)?;
    let passed = verification_output_passed(&filtered, step.is_some(), output.status.success());

    let label = match step {
        Some(step) => format!("Verification script ({step})"),
        None => "Verification script".to_string(),
    };
    let summary = format!(
        "{}\n\n{}",
        if passed {
            "script verification passed"
        } else {
            "script verification failed"
        },
        filtered
    );
    store.add_evidence(
        task_id,
        EvidenceSourceKind::ScriptVerification,
        &script_display,
        &label,
        Some(&summary),
        EvidenceLinkRefs::default(),
    )?;

    let current_task = store.get_task(task_id)?;
    let verification_state = if passed {
        VerificationState::Passed
    } else {
        VerificationState::Failed
    };
    let note = match step {
        Some(step) => format!("verification_script={script_display}; step={step}; passed={passed}"),
        None => format!("verification_script={script_display}; passed={passed}"),
    };
    let mut task = store.update_task_status(
        task_id,
        current_task.status,
        "canopy",
        TaskStatusUpdate {
            verification_state: Some(verification_state),
            event_note: Some(note.as_str()),
            ..TaskStatusUpdate::default()
        },
    )?;

    if passed && task.status != TaskStatus::Completed && store.get_children(task_id)?.is_empty() {
        if !matches!(
            task.status,
            TaskStatus::InProgress | TaskStatus::ReviewRequired
        ) {
            store.update_task_status(
                task_id,
                TaskStatus::InProgress,
                "canopy",
                TaskStatusUpdate {
                    verification_state: Some(VerificationState::Passed),
                    event_note: Some(note.as_str()),
                    ..TaskStatusUpdate::default()
                },
            )?;
        }
        let closure_summary = match step {
            Some(step) => format!("verification passed for {step} via {script_display}"),
            None => format!("verification passed via {script_display}"),
        };
        task = store.update_task_status(
            task_id,
            TaskStatus::Completed,
            "canopy",
            TaskStatusUpdate {
                verification_state: Some(VerificationState::Passed),
                closure_summary: Some(closure_summary.as_str()),
                event_note: Some(note.as_str()),
                ..TaskStatusUpdate::default()
            },
        )?;
    }

    let result = TaskVerificationRun {
        task,
        passed,
        script: script_display,
        step: step.map(ToOwned::to_owned),
        output: filtered,
    };

    if !passed {
        print_json(&result)?;
        anyhow::bail!("verification failed for task {task_id}");
    }

    Ok(result)
}

fn verification_script_command(script: &Path) -> Command {
    #[cfg(windows)]
    {
        if matches!(
            script.extension().and_then(|ext| ext.to_str()),
            Some("cmd" | "bat")
        ) {
            let mut command = Command::new("cmd");
            command.arg("/C").arg(script);
            return command;
        }
    }

    let mut command = Command::new("bash");
    command.arg(script);
    command
}

fn handle_handoff_command(store: &Store, command: HandoffCommand) -> Result<()> {
    match command {
        HandoffCommand::Create {
            task_id,
            from_agent_id,
            to_agent_id,
            handoff_type,
            summary,
            requested_action,
            goal,
            next_steps,
            stop_reason,
            due_at,
            expires_at,
        } => {
            let handoff = store.create_handoff_with_context(
                &task_id,
                &from_agent_id,
                &to_agent_id,
                handoff_type,
                &summary,
                requested_action.as_deref(),
                goal.as_deref(),
                next_steps.as_deref(),
                stop_reason.as_deref(),
                HandoffTiming {
                    due_at: due_at.as_deref(),
                    expires_at: expires_at.as_deref(),
                },
            )?;
            print_json(&handoff)?;
        }
        HandoffCommand::Resolve {
            handoff_id,
            status,
            resolved_by,
            acting_agent_id,
        } => {
            let handoff = store.resolve_handoff_with_actor(
                &handoff_id,
                status,
                &resolved_by,
                acting_agent_id.as_deref(),
            )?;
            print_json(&handoff)?;
        }
        HandoffCommand::Action {
            handoff_id,
            action,
            changed_by,
            acting_agent_id,
            note,
        } => {
            let handoff = store.apply_handoff_operator_action(
                &handoff_id,
                action,
                &changed_by,
                HandoffOperatorActionInput {
                    acting_agent_id: acting_agent_id.as_deref(),
                    note: note.as_deref(),
                },
            )?;
            print_json(&handoff)?;
        }
        HandoffCommand::List { task_id } => {
            let handoffs = store.list_handoffs(task_id.as_deref())?;
            print_json(&handoffs)?;
        }
    }

    Ok(())
}

fn handle_evidence_command(store: &Store, command: EvidenceCommand) -> Result<()> {
    match command {
        EvidenceCommand::Add {
            task_id,
            source_kind,
            source_ref,
            label,
            summary,
            related_handoff_id,
            related_session_id,
            related_memory_query,
            related_symbol,
            related_file,
        } => {
            let fallback_session_id = runtime_session_id_from_env();
            let evidence = store.add_evidence(
                &task_id,
                source_kind,
                &source_ref,
                &label,
                summary.as_deref(),
                EvidenceLinkRefs {
                    related_handoff_id: related_handoff_id.as_deref(),
                    session_id: related_session_id
                        .as_deref()
                        .or(fallback_session_id.as_deref()),
                    memory_query: related_memory_query.as_deref(),
                    symbol: related_symbol.as_deref(),
                    file: related_file.as_deref(),
                },
            )?;
            print_json(&evidence)?;
        }
        EvidenceCommand::List { task_id } => {
            let evidence = store.list_evidence(&task_id)?;
            print_json(&build_evidence_review_rows(&evidence))?;
        }
        EvidenceCommand::Verify { task_id } => {
            print_json(&verify_evidence(store, task_id.as_deref())?)?;
        }
    }

    Ok(())
}

fn combine_command_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    match (stdout.trim(), stderr.trim()) {
        ("", "") => String::new(),
        ("", _) => stderr.into_owned(),
        (_, "") => stdout.into_owned(),
        _ => format!("{stdout}\n{stderr}"),
    }
}

fn filter_verification_output(output: &str, step: Option<&str>) -> Result<String> {
    let Some(step) = step else {
        return Ok(output.trim().to_string());
    };

    let mut in_section = false;
    let mut lines = Vec::new();
    for line in output.lines() {
        if line.starts_with("--- Step ") {
            if line.contains(step) {
                in_section = true;
                lines.push(line.to_string());
                continue;
            }
            if in_section {
                break;
            }
        }
        if in_section {
            lines.push(line.to_string());
        }
    }

    if lines.is_empty() {
        anyhow::bail!("verification output did not contain step '{step}'");
    }

    Ok(lines.join("\n").trim().to_string())
}

fn verification_output_passed(output: &str, step_filtered: bool, exit_success: bool) -> bool {
    match parse_script_verification_status(output) {
        Some(status) => status,
        None if step_filtered => output.contains("PASS:") && !output.contains("FAIL:"),
        None => exit_success && !output.contains("FAIL:"),
    }
}

fn parse_script_verification_status(output: &str) -> Option<bool> {
    for line in output.lines() {
        if let Some((_, failures)) = line.split_once("Results:") {
            let tokens = failures.split_whitespace().collect::<Vec<_>>();
            for window in tokens.windows(2) {
                if window[1].starts_with("failed") {
                    let fail_count = window[0].trim_end_matches(',').parse::<usize>().ok()?;
                    return Some(fail_count == 0);
                }
            }
        }
    }
    None
}

fn verification_script_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("handoff");
    path.with_file_name(format!("verify-{stem}.sh"))
}

fn infer_handoff_project_root(path: &Path) -> Result<String> {
    infer_handoff_workspace_root(path)
        .map(|project_root| project_root.display().to_string())
        .ok_or_else(|| anyhow::anyhow!("failed to infer project root for {}", path.display()))
}

fn infer_handoff_workspace_root(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if ancestor.file_name().and_then(|value| value.to_str()) == Some(".handoffs") {
            return ancestor.parent().map(Path::to_path_buf);
        }
    }

    path.parent().map(Path::to_path_buf)
}

fn extract_handoff_title(content: &str) -> String {
    content
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("# Handoff:") {
                Some(trimmed.trim_start_matches("# Handoff:").trim().to_string())
            } else if trimmed.starts_with("# ") {
                Some(trimmed.trim_start_matches("# ").trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Untitled handoff".to_string())
}

fn extract_handoff_steps(content: &str) -> Vec<ParsedHandoffStep> {
    let mut steps = Vec::new();
    let mut current_marker = None;
    let mut current_body = Vec::new();
    let mut skip_subsection = false;

    let flush_step = |steps: &mut Vec<ParsedHandoffStep>,
                      current_marker: &mut Option<String>,
                      current_body: &mut Vec<String>| {
        if let Some(step_marker) = current_marker.take() {
            let description = current_body.join("\n").trim().to_string();
            let scope = canopy::scope::extract_step_scope(&description);
            steps.push(ParsedHandoffStep {
                description: (!description.is_empty()).then_some(description),
                step_marker,
                scope,
            });
            current_body.clear();
        }
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("### Step ") {
            flush_step(&mut steps, &mut current_marker, &mut current_body);
            let step_marker = trimmed.trim_start_matches("### ").trim().to_string();
            current_marker = Some(step_marker);
            skip_subsection = false;
        } else if current_marker.is_some() && trimmed.starts_with("#### ") {
            skip_subsection = true;
        } else if current_marker.is_some() && !skip_subsection {
            current_body.push(line.to_string());
        }
    }
    flush_step(&mut steps, &mut current_marker, &mut current_body);

    steps
}

fn verify_evidence(store: &Store, task_id: Option<&str>) -> Result<EvidenceVerificationReport> {
    let evidence = if let Some(task_id) = task_id {
        store.list_evidence(task_id)?
    } else {
        store.list_all_evidence()?
    };

    let results = evidence
        .iter()
        .map(|evidence| verify_evidence_ref(evidence, probe_hyphae_session_status))
        .collect();

    Ok(EvidenceVerificationReport {
        schema_version: EVIDENCE_VERIFY_SCHEMA_VERSION.to_string(),
        results,
    })
}

fn verify_evidence_ref<F>(evidence: &EvidenceRef, hyphae_probe: F) -> EvidenceVerificationResult
where
    F: Fn(&str) -> (EvidenceVerificationStatus, String),
{
    let (status, detail) = match evidence.source_kind {
        EvidenceSourceKind::ManualNote => (
            EvidenceVerificationStatus::Verified,
            "manual note is stored directly in canopy".to_string(),
        ),
        EvidenceSourceKind::ScriptVerification => match evidence.summary.as_deref() {
            Some(summary) => match parse_script_verification_status(summary) {
                Some(true) => (
                    EvidenceVerificationStatus::Verified,
                    "verification script reported zero failed checks".to_string(),
                ),
                Some(false) => (
                    EvidenceVerificationStatus::Failed,
                    "verification script reported failing checks".to_string(),
                ),
                None if summary.contains("PASS:") && !summary.contains("FAIL:") => (
                    EvidenceVerificationStatus::Verified,
                    "verification step excerpt contains only passing checks".to_string(),
                ),
                None if summary.contains("FAIL:") => (
                    EvidenceVerificationStatus::Failed,
                    "verification step excerpt contains failing checks".to_string(),
                ),
                None => (
                    EvidenceVerificationStatus::Stale,
                    "verification script evidence is missing parseable results".to_string(),
                ),
            },
            None => (
                EvidenceVerificationStatus::Stale,
                "verification script evidence is missing stored output".to_string(),
            ),
        },
        EvidenceSourceKind::HyphaeSession => {
            let session_id = evidence
                .related_session_id
                .as_deref()
                .or_else(|| non_empty_value(&evidence.source_ref));
            match session_id {
                Some(session_id) => hyphae_probe(session_id),
                None => (
                    EvidenceVerificationStatus::Stale,
                    "hyphae session evidence is missing a session identifier".to_string(),
                ),
            }
        }
        EvidenceSourceKind::HyphaeRecall
        | EvidenceSourceKind::HyphaeOutcome
        | EvidenceSourceKind::CortinaEvent
        | EvidenceSourceKind::MyceliumCommand
        | EvidenceSourceKind::MyceliumExplain
        | EvidenceSourceKind::RhizomeImpact
        | EvidenceSourceKind::RhizomeExport => (
            EvidenceVerificationStatus::Unsupported,
            format!(
                "{} verification is not implemented yet",
                evidence.source_kind
            ),
        ),
    };

    EvidenceVerificationResult {
        evidence_id: evidence.evidence_id.clone(),
        task_id: evidence.task_id.clone(),
        source_kind: evidence.source_kind,
        source_ref: evidence.source_ref.clone(),
        status,
        detail,
    }
}

fn probe_hyphae_session_status(session_id: &str) -> (EvidenceVerificationStatus, String) {
    let span_context = current_span_context().with_tool("hyphae_session_status");
    let _tool_span = tool_span("hyphae_session_status", &span_context).entered();
    let Some(info) = discover(Tool::Hyphae) else {
        return (
            EvidenceVerificationStatus::Unsupported,
            "hyphae binary is not available for session verification".to_string(),
        );
    };

    let _subprocess_span = subprocess_span("hyphae session status", &span_context).entered();
    let output = match Command::new(&info.binary_path)
        .args(["session", "status", "--id", session_id])
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            return (
                EvidenceVerificationStatus::Unsupported,
                format!("failed to execute hyphae session status: {error}"),
            );
        }
    };

    if output.status.success() {
        match serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            Ok(json) if json["session_id"].as_str() == Some(session_id) => (
                EvidenceVerificationStatus::Verified,
                "hyphae session exists".to_string(),
            ),
            Ok(_) => (
                EvidenceVerificationStatus::Stale,
                "hyphae returned a mismatched session payload".to_string(),
            ),
            Err(error) => (
                EvidenceVerificationStatus::Unsupported,
                format!("failed to parse hyphae session status output: {error}"),
            ),
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        if detail.contains("no session with id") {
            (
                EvidenceVerificationStatus::Stale,
                format!("hyphae session '{session_id}' was not found"),
            )
        } else {
            (
                EvidenceVerificationStatus::Unsupported,
                if detail.is_empty() {
                    "hyphae session status failed without stderr output".to_string()
                } else {
                    format!("hyphae session status failed: {detail}")
                },
            )
        }
    }
}

fn non_empty_value(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn runtime_session_id_from_env() -> Option<String> {
    spore::claude_session_id()
}

fn current_span_context() -> SpanContext {
    let context = SpanContext::for_app("canopy");
    match std::env::current_dir() {
        Ok(path) => context.with_workspace_root(path.display().to_string()),
        Err(_) => context,
    }
}

fn command_span_context(cli: &Cli) -> SpanContext {
    let context = current_span_context();
    let workspace_root = match &cli.command {
        Commands::ImportHandoff { path, .. } => infer_handoff_workspace_root(path),
        Commands::Serve { project, .. } => project.as_ref().map(PathBuf::from),
        Commands::WorkQueue { project_root, .. } | Commands::Situation { project_root, .. } => {
            project_root.as_ref().map(PathBuf::from)
        }
        Commands::Task { command } => match command {
            TaskCommand::Create { project_root, .. } => Some(PathBuf::from(project_root)),
            _ => cli
                .db
                .as_ref()
                .and_then(|path| path.parent().map(Path::to_path_buf)),
        },
        Commands::Agent { command } => match command {
            AgentCommand::Register { project_root, .. } => Some(PathBuf::from(project_root)),
            _ => cli
                .db
                .as_ref()
                .and_then(|path| path.parent().map(Path::to_path_buf)),
        },
        _ => cli
            .db
            .as_ref()
            .and_then(|path| path.parent().map(Path::to_path_buf)),
    };

    match workspace_root {
        Some(path) => context.with_workspace_root(path.display().to_string()),
        None => context,
    }
}

fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Agent { .. } => "agent",
        Commands::ImportHandoff { .. } => "import_handoff",
        Commands::Task { .. } => "task",
        Commands::Handoff { .. } => "handoff",
        Commands::Evidence { .. } => "evidence",
        Commands::Council { .. } => "council",
        Commands::Api { .. } => "api",
        Commands::WorkQueue { .. } => "work_queue",
        Commands::Files { .. } => "files",
        Commands::Situation { .. } => "situation",
        Commands::Outcome { .. } => "outcome",
        Commands::Serve { .. } => "serve",
        Commands::Notification { .. } => "notification",
        Commands::Policy { .. } => "policy",
    }
}

/// A serializable participant entry included in the `council_record` artifact.
#[derive(Debug, Serialize)]
struct ArtifactParticipant {
    role: String,
    agent_id: Option<String>,
}

/// Payload emitted to hyphae when a council session is closed.
#[derive(Debug, Serialize)]
struct CouncilRecordArtifact {
    artifact_type: &'static str,
    schema_version: &'static str,
    council_session_id: String,
    task_id: String,
    participants: Vec<ArtifactParticipant>,
    outcome: Option<String>,
    message_count: usize,
    opened_at: String,
    closed_at: String,
}

/// Emits a `council_record` artifact to hyphae after a session is closed.
///
/// Failures are logged as warnings and do not propagate — the close-out flow
/// must not be broken by an unavailable hyphae binary.
fn emit_council_record_artifact(session: &CouncilSession) {
    let participants: Vec<ArtifactParticipant> = session
        .participants
        .iter()
        .map(|p| ArtifactParticipant {
            role: p.role.to_string(),
            agent_id: p.agent_id.clone(),
        })
        .collect();

    let artifact = CouncilRecordArtifact {
        artifact_type: "council_record",
        schema_version: "1.0",
        council_session_id: session.council_session_id.clone(),
        task_id: session.task_id.clone(),
        participants,
        outcome: session.session_summary.clone(),
        message_count: session.timeline.len(),
        opened_at: session.created_at.clone(),
        closed_at: session.updated_at.clone(),
    };

    let payload = match serde_json::to_string(&artifact) {
        Ok(json) => json,
        Err(error) => {
            warn!("council_record artifact serialization failed: {error}");
            return;
        }
    };

    let topic = format!("artifact/council_record/{}", session.council_session_id);

    let Some(info) = discover(Tool::Hyphae) else {
        warn!("hyphae binary not found; skipping council_record artifact emission");
        return;
    };

    let result = Command::new(&info.binary_path)
        .args(["store", "--topic", &topic, "--content", &payload])
        .output();

    match result {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(
                "hyphae store for council_record artifact exited non-zero: {}",
                stderr.trim()
            );
        }
        Err(error) => {
            warn!("failed to invoke hyphae for council_record artifact emission: {error}");
        }
    }
}

fn handle_council_command(store: &Store, command: CouncilCommand) -> Result<()> {
    #[derive(serde::Serialize)]
    struct CouncilSessionStatus {
        council_session_id: String,
        state: String,
        participant_count: usize,
        opened_at: String,
        open_seconds: i64,
    }

    match command {
        CouncilCommand::Summon {
            task_id,
            changed_by,
            transcript_ref,
        } => {
            let session =
                store.summon_task_council(&task_id, &changed_by, transcript_ref.as_deref())?;
            print_json(&session)?;
        }
        CouncilCommand::Post {
            task_id,
            author_agent_id,
            message_type,
            body,
        } => {
            let message =
                store.add_council_message(&task_id, &author_agent_id, message_type, &body)?;
            print_json(&message)?;
        }
        CouncilCommand::Show { task_id } => {
            let messages = store.list_council_messages(&task_id)?;
            print_json(&messages)?;
        }
        CouncilCommand::Open { task } => {
            let session = store.open_council_session(&task)?;
            print_json(&session)?;
        }
        CouncilCommand::Close { session, outcome } => {
            let closed = store.close_council_session(&session, outcome.as_deref())?;
            emit_council_record_artifact(&closed);
            print_json(&closed)?;
        }
        CouncilCommand::Status { task } => {
            let sessions = store.get_open_council_sessions(&task)?;
            let now = chrono::Utc::now();
            let rows: Vec<CouncilSessionStatus> = sessions
                .into_iter()
                .map(|s| {
                    let open_seconds = chrono::DateTime::parse_from_rfc3339(&s.created_at)
                        .map(|ts| (now - ts.with_timezone(&chrono::Utc)).num_seconds())
                        .unwrap_or(0);
                    CouncilSessionStatus {
                        council_session_id: s.council_session_id,
                        state: s.state.to_string(),
                        participant_count: s.participants.len(),
                        opened_at: s.created_at,
                        open_seconds,
                    }
                })
                .collect();
            print_json(&rows)?;
        }
        CouncilCommand::Join { session, agent } => {
            store.join_council_session(&session, &agent)?;
            println!("{{\"ok\":true}}");
        }
    }

    Ok(())
}

fn handle_api_command(store: &Store, command: ApiCommand) -> Result<()> {
    match command {
        ApiCommand::Snapshot {
            project_root,
            preset,
            view,
            sort,
            priority_at_least,
            severity_at_least,
            acknowledged,
            attention_at_least,
        } => {
            print_json(&api::snapshot(
                store,
                api::SnapshotOptions {
                    project_root: project_root.as_deref(),
                    preset,
                    sort,
                    view,
                    priority_at_least,
                    severity_at_least,
                    acknowledged,
                    attention_at_least,
                },
            )?)?;
        }
        ApiCommand::Task { task_id } => {
            print_json(&api::task_detail(store, &task_id)?)?;
        }
    }

    Ok(())
}

fn handle_work_queue(
    store: &Store,
    agent_id: &str,
    limit: i64,
    project_root: Option<&str>,
    json: bool,
) -> Result<()> {
    let agent = store.get_agent(agent_id)?;
    let role = agent.role.as_ref().map(ToString::to_string);
    let capabilities = agent.capabilities.clone();

    let tasks = store.query_available_tasks(role.as_deref(), &capabilities, project_root, limit)?;

    if json {
        print_json(&tasks)?;
    } else {
        print_work_queue_table(&tasks);
    }

    Ok(())
}

fn print_work_queue_table(tasks: &[Task]) {
    if tasks.is_empty() {
        println!("No tasks available");
        return;
    }

    println!("Available tasks ({}):", tasks.len());
    for (idx, task) in tasks.iter().enumerate() {
        let priority_str = task.priority.to_string();
        println!("  {:02}. [{}] {}", idx + 1, priority_str, task.title);
    }
}

fn handle_files_command(store: &Store, command: FilesCommand) -> Result<()> {
    match command {
        FilesCommand::Lock {
            agent_id,
            task_id,
            worktree,
            files,
        } => {
            let conflicts = store.lock_files(&agent_id, &task_id, &files, &worktree)?;
            if conflicts.is_empty() {
                println!("Locked {} files", files.len());
            } else {
                println!("Lock failed - file conflicts:");
                for lock in conflicts {
                    println!("  {} - locked by agent {}", lock.file_path, lock.agent_id);
                }
            }
        }
        FilesCommand::Unlock { task_id } => {
            let count = store.unlock_files(&task_id)?;
            println!("Unlocked {count} files");
        }
        FilesCommand::Check {
            agent_id,
            worktree,
            files,
        } => {
            let conflicts = store.check_file_conflicts(&files, &worktree, agent_id.as_deref())?;
            if conflicts.is_empty() {
                println!("No conflicts - files available");
            } else {
                println!("File conflicts ({}):", conflicts.len());
                for lock in conflicts {
                    println!("  {} - locked by agent {}", lock.file_path, lock.agent_id);
                }
            }
        }
        FilesCommand::List {
            agent_id,
            project_root,
        } => {
            let locks = store.list_file_locks(project_root.as_deref(), agent_id.as_deref())?;
            if locks.is_empty() {
                println!("No active file locks");
            } else {
                println!("Active file locks ({}):", locks.len());
                for lock in locks {
                    println!(
                        "  {} - task {} (agent {})",
                        lock.file_path, lock.task_id, lock.agent_id
                    );
                }
            }
        }
    }

    Ok(())
}

fn handle_situation(
    store: &Store,
    agent_id: Option<&str>,
    project_root: Option<&str>,
) -> Result<()> {
    let agents = if let Some(id) = agent_id {
        vec![store.get_agent(id)?]
    } else {
        store.list_agents()?
    };

    println!("Active agents: {}", agents.len());
    for agent in &agents {
        println!(
            "  {} - {} (status: {})",
            agent.agent_id, agent.model, agent.status
        );
        if let Some(task_id) = &agent.current_task_id {
            if let Ok(task) = store.get_task(task_id) {
                println!("    Current task: {} ({})", task.title, task.status);
            }
        }
    }

    println!();
    let file_locks = store.list_file_locks(project_root, None)?;
    println!("File locks: {}", file_locks.len());
    for lock in &file_locks {
        println!(
            "  {} - task {} (agent {})",
            lock.file_path, lock.task_id, lock.agent_id
        );
    }

    Ok(())
}

/// Handle `canopy outcome <subcommand>`.
///
/// All subcommands are observational — they record and query orchestration
/// outcomes without modifying routing policy.
fn handle_outcome_command(store: &Store, command: OutcomeCommand) -> Result<()> {
    match command {
        OutcomeCommand::Record { path } => {
            let raw: Vec<u8> = if path == "-" {
                use std::io::Read;
                let mut buf = Vec::new();
                std::io::stdin()
                    .read_to_end(&mut buf)
                    .context("reading outcome JSON from stdin")?;
                buf
            } else {
                fs::read(&path).with_context(|| format!("reading outcome JSON from {path}"))?
            };
            let record = store
                .insert_workflow_outcome(&raw)
                .with_context(|| "storing workflow outcome")?;
            print_json(&record)?;
        }
        OutcomeCommand::List => {
            let records = store.list_workflow_outcomes()?;
            print_json(&records)?;
        }
        OutcomeCommand::Show { workflow_id } => {
            let record = store
                .get_workflow_outcome(&workflow_id)?
                .ok_or_else(|| anyhow::anyhow!("outcome not found: {workflow_id}"))?;
            print_json(&record)?;
        }
        OutcomeCommand::Summary => {
            let rows = store.outcome_summary_by_template_failure()?;
            print_json(&rows)?;
        }
    }
    Ok(())
}

fn handle_notification_command(store: &Store, command: NotificationCommand) -> Result<()> {
    match command {
        NotificationCommand::List { all } => {
            let notifs = store.list_notifications(all)?;
            if notifs.is_empty() {
                println!("No notifications.");
            } else {
                for n in &notifs {
                    let read_mark = if n.seen { "[read]" } else { "[unread]" };
                    println!(
                        "{} {} {} {}",
                        read_mark, n.notification_id, n.event_type, n.created_at
                    );
                }
            }
            Ok(())
        }
        NotificationCommand::MarkRead { notification_id } => {
            store.mark_notification_seen(&notification_id)?;
            println!("Marked {notification_id} as read.");
            Ok(())
        }
        NotificationCommand::MarkAllRead => {
            let count = store.mark_all_notifications_seen()?;
            println!("Marked {count} notification(s) as read.");
            Ok(())
        }
    }
}

#[allow(clippy::unnecessary_wraps)]
fn handle_policy_command(command: &PolicyCommand) -> Result<()> {
    match command {
        PolicyCommand::Show => {
            use canopy::tools::policy::DispatchPolicy;
            let desc = DispatchPolicy::Default.describe();
            println!("Active dispatch policy: {}", desc.name);
            println!();
            println!("  readOnlyHint=true    → {}", desc.read_only);
            println!("  destructiveHint=true → {}", desc.destructive);
            println!("  (no hint)            → {}", desc.other);
            Ok(())
        }
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Cli, Commands, EvidenceVerificationStatus, command_span_context, extract_handoff_steps,
        filter_verification_output, parse_script_verification_status, verify_evidence_ref,
    };
    use canopy::models::{EvidenceRef, EvidenceSourceKind};
    use std::path::PathBuf;

    #[test]
    fn verify_manual_note_evidence_as_verified() {
        let evidence = EvidenceRef {
            schema_version: "1.0".to_string(),
            evidence_id: "evidence-1".to_string(),
            task_id: "task-1".to_string(),
            source_kind: EvidenceSourceKind::ManualNote,
            source_ref: "manual://note".to_string(),
            label: "Manual note".to_string(),
            summary: None,
            related_handoff_id: None,
            related_session_id: None,
            related_memory_query: None,
            related_symbol: None,
            related_file: None,
        };

        let result = verify_evidence_ref(&evidence, |_| unreachable!("manual note probe"));
        assert_eq!(result.status, EvidenceVerificationStatus::Verified);
    }

    #[test]
    fn verify_hyphae_session_without_identifier_is_stale() {
        let evidence = EvidenceRef {
            schema_version: "1.0".to_string(),
            evidence_id: "evidence-1".to_string(),
            task_id: "task-1".to_string(),
            source_kind: EvidenceSourceKind::HyphaeSession,
            source_ref: "   ".to_string(),
            label: "Hyphae session".to_string(),
            summary: None,
            related_handoff_id: None,
            related_session_id: None,
            related_memory_query: None,
            related_symbol: None,
            related_file: None,
        };

        let result = verify_evidence_ref(&evidence, |_| unreachable!("missing id probe"));
        assert_eq!(result.status, EvidenceVerificationStatus::Stale);
    }

    #[test]
    fn verify_hyphae_session_uses_probe_result() {
        let evidence = EvidenceRef {
            schema_version: "1.0".to_string(),
            evidence_id: "evidence-1".to_string(),
            task_id: "task-1".to_string(),
            source_kind: EvidenceSourceKind::HyphaeSession,
            source_ref: "session-123".to_string(),
            label: "Hyphae session".to_string(),
            summary: None,
            related_handoff_id: None,
            related_session_id: Some("session-123".to_string()),
            related_memory_query: None,
            related_symbol: None,
            related_file: None,
        };

        let result = verify_evidence_ref(&evidence, |_| {
            (
                EvidenceVerificationStatus::Unsupported,
                "hyphae unavailable".to_string(),
            )
        });
        assert_eq!(result.status, EvidenceVerificationStatus::Unsupported);
        assert!(result.detail.contains("hyphae unavailable"));
    }

    #[test]
    fn verify_script_verification_evidence_reports_failures() {
        let evidence = EvidenceRef {
            schema_version: "1.0".to_string(),
            evidence_id: "evidence-2".to_string(),
            task_id: "task-2".to_string(),
            source_kind: EvidenceSourceKind::ScriptVerification,
            source_ref: "/tmp/verify.sh".to_string(),
            label: "Verification script".to_string(),
            summary: Some("script verification failed\n\nResults: 3 passed, 1 failed".to_string()),
            related_handoff_id: None,
            related_session_id: None,
            related_memory_query: None,
            related_symbol: None,
            related_file: None,
        };

        let result = verify_evidence_ref(&evidence, |_| unreachable!("script evidence probe"));
        assert_eq!(result.status, EvidenceVerificationStatus::Failed);
    }

    #[test]
    fn filter_verification_output_extracts_one_step_section() {
        let output = "\
=== Verify ===
--- Step 1: Alpha ---
  PASS: alpha
--- Step 2: Beta ---
  FAIL: beta
Results: 1 passed, 1 failed";

        let filtered =
            filter_verification_output(output, Some("Step 1")).expect("extract step section");
        assert!(filtered.contains("PASS: alpha"));
        assert!(!filtered.contains("FAIL: beta"));
    }

    #[test]
    fn parse_script_verification_status_reads_results_line() {
        assert_eq!(
            parse_script_verification_status("Results: 4 passed, 0 failed"),
            Some(true)
        );
        assert_eq!(
            parse_script_verification_status("Results: 4 passed, 2 failed"),
            Some(false)
        );
    }

    #[test]
    fn extract_handoff_steps_reads_step_sections() {
        let content = "\
# Handoff: Example

### Step 1: First
Implement the first step.

#### Verification
ignore this

### Step 2: Second
Implement the second step.
";

        let steps = extract_handoff_steps(content);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].step_marker, "Step 1: First");
        assert_eq!(
            steps[0].description.as_deref(),
            Some("Implement the first step.")
        );
        assert_eq!(steps[1].step_marker, "Step 2: Second");
    }

    #[test]
    fn council_record_artifact_payload_is_well_formed() {
        use canopy::models::{
            CouncilParticipant, CouncilParticipantRole, CouncilParticipantStatus, CouncilSession,
            CouncilSessionState,
        };

        let session = CouncilSession {
            council_session_id: "cs-01JTEST".to_string(),
            task_id: "task-01JTEST".to_string(),
            worktree_id: None,
            participants: vec![
                CouncilParticipant {
                    role: CouncilParticipantRole::Reviewer,
                    agent_id: Some("agent-alpha".to_string()),
                    status: Some(CouncilParticipantStatus::Summoned),
                },
                CouncilParticipant {
                    role: CouncilParticipantRole::Architect,
                    agent_id: None,
                    status: Some(CouncilParticipantStatus::Summoned),
                },
            ],
            session_summary: Some("Approved with minor notes.".to_string()),
            state: CouncilSessionState::Closed,
            timeline: vec![],
            transcript_ref: None,
            created_at: "2026-04-15T10:00:00Z".to_string(),
            updated_at: "2026-04-15T11:00:00Z".to_string(),
        };

        // Build the artifact the same way emit_council_record_artifact does.
        let participants: Vec<super::ArtifactParticipant> = session
            .participants
            .iter()
            .map(|p| super::ArtifactParticipant {
                role: p.role.to_string(),
                agent_id: p.agent_id.clone(),
            })
            .collect();

        let artifact = super::CouncilRecordArtifact {
            artifact_type: "council_record",
            schema_version: "1.0",
            council_session_id: session.council_session_id.clone(),
            task_id: session.task_id.clone(),
            participants,
            outcome: session.session_summary.clone(),
            message_count: session.timeline.len(),
            opened_at: session.created_at.clone(),
            closed_at: session.updated_at.clone(),
        };

        let json = serde_json::to_string(&artifact).expect("artifact must serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("must be valid JSON");

        assert_eq!(value["artifact_type"], "council_record");
        assert_eq!(value["schema_version"], "1.0");
        assert_eq!(value["council_session_id"], "cs-01JTEST");
        assert_eq!(value["task_id"], "task-01JTEST");
        assert_eq!(value["outcome"], "Approved with minor notes.");
        assert_eq!(value["message_count"], 0);
        assert_eq!(value["opened_at"], "2026-04-15T10:00:00Z");
        assert_eq!(value["closed_at"], "2026-04-15T11:00:00Z");

        let parts = value["participants"]
            .as_array()
            .expect("participants array");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["role"], "reviewer");
        assert_eq!(parts[0]["agent_id"], "agent-alpha");
        assert_eq!(parts[1]["role"], "architect");
        assert!(parts[1]["agent_id"].is_null());
    }

    #[test]
    fn import_handoff_command_uses_project_root_workspace_context() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path().join("workspace");
        let handoff_dir = project_root.join(".handoffs").join("canopy");
        std::fs::create_dir_all(&handoff_dir).unwrap();
        let handoff_path = handoff_dir.join("demo.md");

        let cli = Cli {
            db: None,
            command: Commands::ImportHandoff {
                path: PathBuf::from(&handoff_path),
                assign: None,
            },
        };
        let context = command_span_context(&cli);
        let project_root = project_root.display().to_string();

        assert_eq!(
            context.workspace_root.as_deref(),
            Some(project_root.as_str())
        );
    }
}
