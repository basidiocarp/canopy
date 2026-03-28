use anyhow::{Context, Result};
use canopy::api;
use canopy::cli::{
    AgentCommand, ApiCommand, Cli, Commands, CouncilCommand, EvidenceCommand, HandoffCommand,
    TaskCommand,
};
use canopy::models::{AgentRegistration, AgentStatus};
use canopy::store::EvidenceLinkRefs;
use canopy::store::Store;
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
                verification_state,
                blocked_reason.as_deref(),
                closure_summary.as_deref(),
            )?;
            print_json(&task)?;
        }
        TaskCommand::List => {
            print_json(&store.list_tasks()?)?;
        }
        TaskCommand::ListView {
            project_root,
            view,
            sort,
        } => {
            let snapshot = api::snapshot(
                store,
                api::SnapshotOptions {
                    project_root: project_root.as_deref(),
                    sort,
                    view,
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
        } => {
            let handoff = store.create_handoff(
                &task_id,
                &from_agent_id,
                &to_agent_id,
                handoff_type,
                &summary,
                requested_action.as_deref(),
            )?;
            print_json(&handoff)?;
        }
        HandoffCommand::Resolve { handoff_id, status } => {
            let handoff = store.resolve_handoff(&handoff_id, status)?;
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
            view,
            sort,
        } => {
            print_json(&api::snapshot(
                store,
                api::SnapshotOptions {
                    project_root: project_root.as_deref(),
                    sort,
                    view,
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
