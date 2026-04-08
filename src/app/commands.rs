#![allow(clippy::wildcard_imports)]

use super::server;
use super::*;

pub(super) fn run(store: &Store, command: Commands) -> Result<()> {
    match command {
        Commands::Agent { command } => handle_agent_command(store, command)?,
        Commands::ImportHandoff { path, assign } => {
            handle_import_handoff(store, &path, assign.as_deref())?;
        }
        Commands::Task { command } => handle_task_command(store, command)?,
        Commands::Handoff { command } => handle_handoff_command(store, command)?,
        Commands::Evidence { command } => handle_evidence_command(store, command)?,
        Commands::Council { command } => handle_council_command(store, command)?,
        Commands::Api { command } => handle_api_command(store, command)?,
        Commands::WorkQueue {
            agent_id,
            limit,
            project_root,
            json,
        } => {
            handle_work_queue(store, &agent_id, limit, project_root.as_deref(), json)?;
        }
        Commands::Files { command } => handle_files_command(store, command)?,
        Commands::Situation {
            agent_id,
            project_root,
        } => {
            handle_situation(store, agent_id.as_deref(), project_root.as_deref())?;
        }
        Commands::Serve {
            agent_id,
            project,
            worktree,
        } => {
            server::run(store, &agent_id, project.as_deref(), &worktree)?;
        }
    }

    Ok(())
}
