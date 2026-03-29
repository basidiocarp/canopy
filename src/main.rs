use anyhow::{Context, Result};
use canopy::api;
use canopy::cli::{
    AgentCommand, ApiCommand, Cli, Commands, CouncilCommand, EvidenceCommand, HandoffCommand,
    TaskCommand,
};
use canopy::models::{AgentRegistration, AgentStatus};
use canopy::store::{
    EvidenceLinkRefs, HandoffOperatorActionInput, HandoffTiming, Store, TaskOperatorActionInput,
    TaskStatusUpdate, TaskTriageUpdate,
};
use clap::Parser;
use serde::Serialize;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::open(&resolve_db_path(cli.db.as_deref())?).context("open canopy store")?;
    run_command(&store, cli.command)
}

fn run_command(store: &Store, command: Commands) -> Result<()> {
    match command {
        Commands::Agent { command } => handle_agent_command(store, command)?,
        Commands::Task { command } => handle_task_command(store, command)?,
        Commands::Handoff { command } => handle_handoff_command(store, command)?,
        Commands::Evidence { command } => handle_evidence_command(store, command)?,
        Commands::Council { command } => handle_council_command(store, command)?,
        Commands::Api { command } => handle_api_command(store, command)?,
    }

    Ok(())
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
        } => {
            let agent = AgentRegistration {
                agent_id,
                host_id,
                host_type,
                host_instance,
                model,
                project_root,
                worktree_id,
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
            print_json(&store.list_agents()?)?;
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
        } => {
            let task =
                store.create_task(&title, description.as_deref(), &requested_by, &project_root)?;
            print_json(&task)?;
        }
        TaskCommand::Assign {
            task_id,
            assigned_to,
            assigned_by,
            reason,
        } => {
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
            let task = store.apply_task_operator_action(
                &task_id,
                action,
                &changed_by,
                TaskOperatorActionInput {
                    acting_agent_id: acting_agent_id.as_deref(),
                    assigned_to: assigned_to.as_deref(),
                    priority,
                    severity,
                    verification_state,
                    blocked_reason: blocked_reason.as_deref(),
                    closure_summary: closure_summary.as_deref(),
                    owner_note: owner_note.as_deref(),
                    clear_owner_note,
                    note: note.as_deref(),
                    from_agent_id: from_agent_id.as_deref(),
                    to_agent_id: to_agent_id.as_deref(),
                    handoff_type,
                    handoff_summary: handoff_summary.as_deref(),
                    requested_action: requested_action.as_deref(),
                    due_at: due_at.as_deref(),
                    review_due_at: review_due_at.as_deref(),
                    expires_at: expires_at.as_deref(),
                    author_agent_id: author_agent_id.as_deref(),
                    message_type,
                    message_body: message_body.as_deref(),
                    evidence_source_kind,
                    evidence_source_ref: evidence_source_ref.as_deref(),
                    evidence_label: evidence_label.as_deref(),
                    evidence_summary: evidence_summary.as_deref(),
                    related_handoff_id: related_handoff_id.as_deref(),
                    related_session_id: related_session_id.as_deref(),
                    related_memory_query: related_memory_query.as_deref(),
                    related_symbol: related_symbol.as_deref(),
                    related_file: related_file.as_deref(),
                    follow_up_title: follow_up_title.as_deref(),
                    follow_up_description: follow_up_description.as_deref(),
                    related_task_id: related_task_id.as_deref(),
                    relationship_role,
                },
            )?;
            print_json(&task)?;
        }
        TaskCommand::List => {
            print_json(&store.list_tasks()?)?;
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
    }

    Ok(())
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
            due_at,
            expires_at,
        } => {
            let handoff = store.create_handoff(
                &task_id,
                &from_agent_id,
                &to_agent_id,
                handoff_type,
                &summary,
                requested_action.as_deref(),
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
            let evidence = store.add_evidence(
                &task_id,
                source_kind,
                &source_ref,
                &label,
                summary.as_deref(),
                EvidenceLinkRefs {
                    related_handoff_id: related_handoff_id.as_deref(),
                    session_id: related_session_id.as_deref(),
                    memory_query: related_memory_query.as_deref(),
                    symbol: related_symbol.as_deref(),
                    file: related_file.as_deref(),
                },
            )?;
            print_json(&evidence)?;
        }
        EvidenceCommand::List { task_id } => {
            print_json(&store.list_evidence(&task_id)?)?;
        }
    }

    Ok(())
}

fn handle_council_command(store: &Store, command: CouncilCommand) -> Result<()> {
    match command {
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

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn resolve_db_path(db: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = db {
        return Ok(path.to_path_buf());
    }

    let state_dir = PathBuf::from(".canopy");
    std::fs::create_dir_all(&state_dir).context("create .canopy state directory")?;
    Ok(state_dir.join("canopy.db"))
}
