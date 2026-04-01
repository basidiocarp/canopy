use anyhow::{Context, Result};
use canopy::api;
use canopy::cli::{
    AgentCommand, ApiCommand, Cli, Commands, CouncilCommand, EvidenceCommand, HandoffCommand,
    TaskCommand,
};
use canopy::models::{
    AgentRegistration, AgentStatus, EvidenceRef, EvidenceSourceKind, EvidenceVerificationReport,
    EvidenceVerificationResult, EvidenceVerificationStatus,
};
use canopy::store::{
    EvidenceLinkRefs, HandoffOperatorActionInput, HandoffTiming, Store, TaskCreationOptions,
    TaskOperatorActionInput, TaskStatusUpdate, TaskTriageUpdate,
};
use clap::Parser;
use serde::Serialize;
use spore::{Tool, discover};
use std::path::{Path, PathBuf};
use std::process::Command;

const CANOPY_DB_FILENAME: &str = "canopy.db";
const CANOPY_DB_ENV_VAR: &str = "CANOPY_DB_PATH";
const EVIDENCE_VERIFY_SCHEMA_VERSION: &str = "1.0";

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
            parent,
            required_role,
            required_capabilities,
            auto_review,
        } => {
            let options = TaskCreationOptions {
                required_role,
                required_capabilities,
                auto_review,
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
                    related_session_id: related_session_id
                        .as_deref()
                        .or(fallback_session_id.as_deref()),
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
            print_json(&store.list_evidence(&task_id)?)?;
        }
        EvidenceCommand::Verify { task_id } => {
            print_json(&verify_evidence(store, task_id.as_deref())?)?;
        }
    }

    Ok(())
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
    let Some(info) = discover(Tool::Hyphae) else {
        return (
            EvidenceVerificationStatus::Unsupported,
            "hyphae binary is not available for session verification".to_string(),
        );
    };

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
    std::env::var("CLAUDE_SESSION_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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

    let target = spore::paths::db_path("canopy", CANOPY_DB_FILENAME, CANOPY_DB_ENV_VAR, None)
        .context("resolve canopy database path")?;
    migrate_legacy_db_if_needed(&PathBuf::from(".canopy").join(CANOPY_DB_FILENAME), &target)?;
    Ok(target)
}

fn migrate_legacy_db_if_needed(legacy_path: &Path, target_path: &Path) -> Result<()> {
    if legacy_path == target_path || !legacy_path.exists() || target_path.exists() {
        return Ok(());
    }

    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create canopy data directory {}", parent.display()))?;
    }

    match std::fs::rename(legacy_path, target_path) {
        Ok(()) => {}
        Err(rename_err) => {
            std::fs::copy(legacy_path, target_path).with_context(|| {
                format!(
                    "copy legacy canopy database from {} to {} after rename failed: {rename_err}",
                    legacy_path.display(),
                    target_path.display()
                )
            })?;
            std::fs::remove_file(legacy_path).with_context(|| {
                format!(
                    "remove migrated legacy canopy database {}",
                    legacy_path.display()
                )
            })?;
        }
    }

    if let Some(legacy_dir) = legacy_path.parent() {
        let _ = std::fs::remove_dir(legacy_dir);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EvidenceVerificationStatus, migrate_legacy_db_if_needed, verify_evidence_ref};
    use canopy::models::{EvidenceRef, EvidenceSourceKind};
    use tempfile::tempdir;

    #[test]
    fn migrate_legacy_db_moves_existing_db_to_spore_target() {
        let temp = tempdir().expect("temp dir");
        let legacy_dir = temp.path().join(".canopy");
        std::fs::create_dir_all(&legacy_dir).expect("legacy dir");
        let legacy_db = legacy_dir.join("canopy.db");
        let target_db = temp.path().join("state").join("canopy.db");

        std::fs::write(&legacy_db, "legacy").expect("write legacy db");

        migrate_legacy_db_if_needed(&legacy_db, &target_db).expect("migrate legacy db");

        assert!(!legacy_db.exists());
        assert_eq!(
            std::fs::read_to_string(&target_db).expect("read target db"),
            "legacy"
        );
    }

    #[test]
    fn migrate_legacy_db_leaves_existing_target_untouched() {
        let temp = tempdir().expect("temp dir");
        let legacy_dir = temp.path().join(".canopy");
        let target_dir = temp.path().join("state");
        std::fs::create_dir_all(&legacy_dir).expect("legacy dir");
        std::fs::create_dir_all(&target_dir).expect("target dir");

        let legacy_db = legacy_dir.join("canopy.db");
        let target_db = target_dir.join("canopy.db");
        std::fs::write(&legacy_db, "legacy").expect("write legacy db");
        std::fs::write(&target_db, "current").expect("write target db");

        migrate_legacy_db_if_needed(&legacy_db, &target_db).expect("skip migration");

        assert_eq!(
            std::fs::read_to_string(&legacy_db).expect("read legacy db"),
            "legacy"
        );
        assert_eq!(
            std::fs::read_to_string(&target_db).expect("read target db"),
            "current"
        );
    }

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
}
