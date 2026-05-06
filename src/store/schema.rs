use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

use super::StoreResult;

pub(crate) const BASE_SCHEMA: &str = r"
    CREATE TABLE IF NOT EXISTS agents (
        agent_id TEXT PRIMARY KEY,
        host_id TEXT NOT NULL,
        host_type TEXT NOT NULL,
        host_instance TEXT NOT NULL,
        model TEXT NOT NULL,
        project_root TEXT NOT NULL,
        worktree_id TEXT NOT NULL,
        role TEXT NULL,
        capabilities TEXT NOT NULL DEFAULT '[]',
        status TEXT NOT NULL,
        current_task_id TEXT NULL,
        heartbeat_at TEXT NULL,
        tier TEXT NULL,
        specializations TEXT NOT NULL DEFAULT '[]',
        last_heartbeat_at TEXT NULL
    );

    CREATE TABLE IF NOT EXISTS tasks (
        task_id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        description TEXT NULL,
        requested_by TEXT NOT NULL,
        project_root TEXT NOT NULL,
        workspace TEXT NULL,
        parent_task_id TEXT NULL REFERENCES tasks(task_id) ON DELETE SET NULL,
        queue_state_id TEXT NULL,
        worktree_binding_id TEXT NULL,
        execution_session_ref TEXT NULL,
        review_cycle_id TEXT NULL,
        workflow_id TEXT NULL,
        phase_id TEXT NULL,
        required_role TEXT NULL,
        required_capabilities TEXT NOT NULL DEFAULT '[]',
        auto_review INTEGER NOT NULL DEFAULT 0,
        verification_required INTEGER NOT NULL DEFAULT 0,
        status TEXT NOT NULL,
        verification_state TEXT NOT NULL,
        priority TEXT NOT NULL,
        severity TEXT NOT NULL,
        owner_agent_id TEXT NULL REFERENCES agents(agent_id),
        owner_note TEXT NULL,
        acknowledged_by TEXT NULL,
        acknowledged_at TEXT NULL,
        blocked_reason TEXT NULL,
        verified_by TEXT NULL,
        verified_at TEXT NULL,
        closed_by TEXT NULL,
        closure_summary TEXT NULL,
        closed_at TEXT NULL,
        due_at TEXT NULL,
        review_due_at TEXT NULL,
        scope TEXT NOT NULL DEFAULT '[]',
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        output TEXT NULL,
        claimed_at TEXT NULL,
        files_hint TEXT NULL
    );

    CREATE TABLE IF NOT EXISTS task_queue_states (
        queue_state_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL UNIQUE REFERENCES tasks(task_id) ON DELETE CASCADE,
        queue_name TEXT NOT NULL,
        lane TEXT NOT NULL,
        position INTEGER NOT NULL DEFAULT 0,
        status TEXT NOT NULL,
        owner_agent_id TEXT NULL REFERENCES agents(agent_id),
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS task_worktree_bindings (
        worktree_binding_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL UNIQUE REFERENCES tasks(task_id) ON DELETE CASCADE,
        project_root TEXT NOT NULL,
        agent_id TEXT NULL REFERENCES agents(agent_id),
        worktree_id TEXT NULL,
        execution_session_ref TEXT NULL,
        status TEXT NOT NULL,
        bound_at TEXT NULL,
        released_at TEXT NULL,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS task_review_cycles (
        review_cycle_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        cycle_number INTEGER NOT NULL DEFAULT 1,
        state TEXT NOT NULL,
        council_session_id TEXT NULL REFERENCES council_sessions(council_session_id) ON DELETE SET NULL,
        requested_by TEXT NULL,
        evidence_count INTEGER NOT NULL DEFAULT 0,
        decision_count INTEGER NOT NULL DEFAULT 0,
        opened_at TEXT NULL,
        decided_at TEXT NULL,
        closed_at TEXT NULL,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        UNIQUE(task_id, cycle_number)
    );

    CREATE TABLE IF NOT EXISTS task_assignments (
        assignment_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        assigned_to TEXT NOT NULL REFERENCES agents(agent_id),
        assigned_by TEXT NOT NULL,
        reason TEXT NULL,
        assigned_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS handoffs (
        handoff_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        from_agent_id TEXT NOT NULL REFERENCES agents(agent_id),
        to_agent_id TEXT NOT NULL REFERENCES agents(agent_id),
        handoff_type TEXT NOT NULL,
        summary TEXT NOT NULL,
        requested_action TEXT NULL,
        goal TEXT NULL,
        next_steps TEXT NULL,
        stop_reason TEXT NULL,
        due_at TEXT NULL,
        expires_at TEXT NULL,
        status TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        resolved_at TEXT NULL
    );

    CREATE TABLE IF NOT EXISTS council_messages (
        message_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        author_agent_id TEXT NOT NULL REFERENCES agents(agent_id),
        message_type TEXT NOT NULL,
        body TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS council_sessions (
        council_session_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL UNIQUE REFERENCES tasks(task_id) ON DELETE CASCADE,
        project_root TEXT NOT NULL,
        worktree_id TEXT NULL,
        participants_json TEXT NOT NULL DEFAULT '[]',
        state TEXT NOT NULL,
        session_summary TEXT NULL,
        transcript_ref TEXT NULL,
        timeline_ref TEXT NOT NULL,
        opened_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        closed_at TEXT NULL,
        conversation_id TEXT NULL
    );

    CREATE TABLE IF NOT EXISTS evidence_refs (
        schema_version TEXT NOT NULL DEFAULT '1.0',
        evidence_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        source_kind TEXT NOT NULL,
        source_ref TEXT NOT NULL,
        label TEXT NOT NULL,
        summary TEXT NULL,
        related_handoff_id TEXT NULL REFERENCES handoffs(handoff_id),
        related_session_id TEXT NULL,
        related_memory_query TEXT NULL,
        related_symbol TEXT NULL,
        related_file TEXT NULL
    );

    CREATE TABLE IF NOT EXISTS task_events (
        event_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        event_type TEXT NOT NULL,
        actor TEXT NOT NULL,
        from_status TEXT NULL,
        to_status TEXT NOT NULL,
        verification_state TEXT NULL,
        owner_agent_id TEXT NULL,
        execution_action TEXT NULL,
        execution_duration_seconds INTEGER NULL,
        note TEXT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS task_relationships (
        relationship_id TEXT PRIMARY KEY,
        source_task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        target_task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        kind TEXT NOT NULL,
        created_by TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        UNIQUE(source_task_id, target_task_id, kind)
    );

    CREATE UNIQUE INDEX IF NOT EXISTS idx_task_relationships_parent_source
    ON task_relationships(source_task_id)
    WHERE kind = 'parent';

    CREATE INDEX IF NOT EXISTS idx_tasks_parent_task_id ON tasks(parent_task_id);

    CREATE TABLE IF NOT EXISTS agent_heartbeat_events (
        heartbeat_id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL REFERENCES agents(agent_id) ON DELETE CASCADE,
        status TEXT NOT NULL,
        current_task_id TEXT NULL REFERENCES tasks(task_id) ON DELETE SET NULL,
        related_task_id TEXT NULL REFERENCES tasks(task_id) ON DELETE SET NULL,
        source TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS file_locks (
        lock_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
        agent_id TEXT NOT NULL,
        file_path TEXT NOT NULL,
        worktree_id TEXT NOT NULL,
        locked_at TEXT NOT NULL,
        released_at TEXT
    );

    CREATE UNIQUE INDEX IF NOT EXISTS idx_file_locks_active
        ON file_locks(file_path, worktree_id) WHERE released_at IS NULL;
    CREATE INDEX IF NOT EXISTS idx_file_locks_agent
        ON file_locks(agent_id) WHERE released_at IS NULL;
    CREATE INDEX IF NOT EXISTS idx_file_locks_task
        ON file_locks(task_id) WHERE released_at IS NULL;

    -- Orchestration outcome learning loop (#141g).
    -- Observational only: records what happened so policy review has a
    -- truthful baseline. Does not auto-modify routing policy.
    CREATE TABLE IF NOT EXISTS workflow_outcomes (
        workflow_id          TEXT PRIMARY KEY,
        template_id          TEXT NOT NULL,
        handoff_path         TEXT NOT NULL,
        terminal_status      TEXT NOT NULL,
        failure_type         TEXT NULL,
        attempt_count        INTEGER NOT NULL,
        route_taken_json     TEXT NOT NULL,
        confidence           REAL NULL,
        root_cause_layer     TEXT NULL,
        runtime_identity_json TEXT NULL,
        started_at           TEXT NOT NULL,
        completed_at         TEXT NOT NULL,
        created_at           TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE INDEX IF NOT EXISTS idx_workflow_outcomes_template_failure
        ON workflow_outcomes(template_id, failure_type);

    CREATE TABLE IF NOT EXISTS notifications (
        notification_id TEXT NOT NULL PRIMARY KEY,
        event_type      TEXT NOT NULL,
        task_id         TEXT,
        agent_id        TEXT,
        payload         TEXT NOT NULL DEFAULT '{}',
        seen            INTEGER NOT NULL DEFAULT 0,
        created_at      TEXT NOT NULL,
        FOREIGN KEY (task_id) REFERENCES tasks(task_id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_notifications_task ON notifications(task_id);
    CREATE INDEX IF NOT EXISTS idx_notifications_seen ON notifications(seen);

    CREATE TABLE IF NOT EXISTS tool_adoption_scores (
        task_id    TEXT NOT NULL PRIMARY KEY,
        score_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY (task_id) REFERENCES tasks(task_id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS policy_events (
        event_id    TEXT PRIMARY KEY,
        ts          INTEGER NOT NULL,
        agent_id    TEXT NOT NULL,
        tool_name   TEXT NOT NULL,
        decision    TEXT NOT NULL CHECK(decision IN ('proceed', 'flag')),
        reason      TEXT NOT NULL,
        task_id     TEXT
    );

    CREATE TABLE IF NOT EXISTS dag_graphs (
        graph_id    TEXT PRIMARY KEY,
        name        TEXT NOT NULL,
        status      TEXT NOT NULL DEFAULT 'open'
                        CHECK(status IN ('open', 'complete', 'failed')),
        created_at  INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS dag_nodes (
        node_id      TEXT PRIMARY KEY,
        graph_id     TEXT NOT NULL REFERENCES dag_graphs(graph_id),
        label        TEXT NOT NULL,
        status       TEXT NOT NULL DEFAULT 'pending'
                         CHECK(status IN ('pending', 'ready', 'running', 'complete', 'failed')),
        task_id      TEXT,
        created_at   INTEGER NOT NULL,
        completed_at INTEGER
    );

    CREATE TABLE IF NOT EXISTS dag_edges (
        edge_id      TEXT PRIMARY KEY,
        graph_id     TEXT NOT NULL REFERENCES dag_graphs(graph_id),
        from_node_id TEXT NOT NULL REFERENCES dag_nodes(node_id),
        to_node_id   TEXT NOT NULL REFERENCES dag_nodes(node_id),
        edge_type    TEXT NOT NULL DEFAULT 'blocks'
                         CHECK(edge_type IN ('blocks', 'informs'))
    );

    CREATE INDEX IF NOT EXISTS idx_dag_nodes_graph ON dag_nodes(graph_id);
    CREATE INDEX IF NOT EXISTS idx_dag_edges_to ON dag_edges(to_node_id);

    CREATE TABLE IF NOT EXISTS permission_rules (
        rule_id     TEXT PRIMARY KEY,
        agent_id    TEXT NOT NULL,
        tool_name   TEXT NOT NULL,
        action      TEXT NOT NULL CHECK(action IN ('allow', 'deny')),
        scope       TEXT NOT NULL CHECK(scope IN ('session', 'permanent')),
        reason      TEXT NOT NULL DEFAULT '',
        created_at  INTEGER NOT NULL,
        expires_at  INTEGER
    );

    CREATE INDEX IF NOT EXISTS idx_permission_rules_lookup
        ON permission_rules(agent_id, tool_name);

    -- Known-facts cache (H-09). Populated by EstablishedFact events from agents.
    -- Provides cheap pre-Hyphae lookup: hit → load by hyphae_id; miss → full search.
    CREATE TABLE IF NOT EXISTS known_facts (
        fact_id          TEXT PRIMARY KEY,
        key              TEXT NOT NULL,
        fact_type        TEXT NOT NULL DEFAULT 'other',
        scope            TEXT NOT NULL DEFAULT 'project',
        summary          TEXT NOT NULL,
        hyphae_id        TEXT,
        established_by   TEXT NOT NULL,
        task_id          TEXT REFERENCES tasks(task_id) ON DELETE SET NULL,
        confidence       REAL NOT NULL DEFAULT 1.0,
        established_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE INDEX IF NOT EXISTS idx_known_facts_key ON known_facts(key);
    CREATE INDEX IF NOT EXISTS idx_known_facts_task ON known_facts(task_id);

    -- Keep tasks.parent_task_id and task_relationships in sync on parent deletion.
    -- When a parent task is deleted, SQLite's FK engine sets children's parent_task_id
    -- to NULL (ON DELETE SET NULL) and cascade-deletes task_relationships rows
    -- (ON DELETE CASCADE). Both happen atomically in the same FK pass, but this
    -- trigger makes the intent explicit and guards against any ordering ambiguity.
    CREATE TRIGGER IF NOT EXISTS trg_task_delete_clear_parent_relationship
    BEFORE DELETE ON tasks
    FOR EACH ROW
    BEGIN
        -- Explicitly remove the task_relationships rows that record the parent link
        -- for any child that points to the task being deleted, so the two sources
        -- of truth (tasks.parent_task_id and task_relationships) stay in sync even
        -- if the FK cascade order ever changes.
        DELETE FROM task_relationships
        WHERE kind = 'parent'
          AND target_task_id = OLD.task_id;
    END;

    -- Review annotations from the cap inline diff review UI (inline-diff-review handoff).
    CREATE TABLE IF NOT EXISTS review_annotations (
        annotation_id  TEXT PRIMARY KEY,
        task_id        TEXT NOT NULL REFERENCES tasks(task_id),
        file_path      TEXT NOT NULL,
        start_line     INTEGER NOT NULL,
        end_line       INTEGER NOT NULL,
        action         TEXT NOT NULL CHECK(action IN ('approve', 'reject', 'revise')),
        comment        TEXT NOT NULL DEFAULT '',
        anchor_hash    TEXT NOT NULL,
        operator_id    TEXT NOT NULL,
        created_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE INDEX IF NOT EXISTS idx_review_annotations_task
        ON review_annotations(task_id);
";

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(BASE_SCHEMA),
        M::up(r"
            CREATE TABLE IF NOT EXISTS review_annotations (
                annotation_id  TEXT PRIMARY KEY,
                task_id        TEXT NOT NULL REFERENCES tasks(task_id),
                file_path      TEXT NOT NULL,
                start_line     INTEGER NOT NULL,
                end_line       INTEGER NOT NULL,
                action         TEXT NOT NULL CHECK(action IN ('approve', 'reject', 'revise')),
                comment        TEXT NOT NULL DEFAULT '',
                anchor_hash    TEXT NOT NULL,
                operator_id    TEXT NOT NULL,
                created_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_review_annotations_task
                ON review_annotations(task_id);
        "),
    ])
}

fn bootstrap_existing_db(conn: &Connection) -> rusqlite::Result<()> {
    let user_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if user_version != 0 {
        return Ok(());
    }
    let tasks_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='tasks'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .is_ok_and(|c| c > 0);
    if !tasks_exists {
        // Fresh database: leave user_version at 0 so to_latest() runs M0.
        return Ok(());
    }

    // Add missing columns to tables that existed before these columns were introduced.
    // ALTER TABLE fails with "duplicate column name" if the column already exists;
    // that is the expected case on a fully up-to-date database, so we intentionally discard.
    // Column patches must run BEFORE execute_batch(BASE_SCHEMA) because BASE_SCHEMA includes
    // indexes that reference these columns; those indexes would fail if columns don't exist yet.

    // tasks columns added over time
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN priority TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN severity TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN required_role TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN required_capabilities TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN auto_review INTEGER NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN verification_required INTEGER NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN owner_note TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN acknowledged_by TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN acknowledged_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN due_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN review_due_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN parent_task_id TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN queue_state_id TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN worktree_binding_id TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN execution_session_ref TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN review_cycle_id TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN created_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN updated_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN output TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN scope TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN claimed_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN files_hint TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN workflow_id TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN phase_id TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE tasks ADD COLUMN workspace TEXT NULL", []);

    // handoffs columns added over time
    let _ = conn.execute("ALTER TABLE handoffs ADD COLUMN due_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE handoffs ADD COLUMN expires_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE handoffs ADD COLUMN created_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE handoffs ADD COLUMN updated_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE handoffs ADD COLUMN resolved_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE handoffs ADD COLUMN goal TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE handoffs ADD COLUMN next_steps TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE handoffs ADD COLUMN stop_reason TEXT NULL", []);

    // agents columns added over time
    let _ = conn.execute("ALTER TABLE agents ADD COLUMN role TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE agents ADD COLUMN capabilities TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE agents ADD COLUMN tier TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE agents ADD COLUMN specializations TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE agents ADD COLUMN last_heartbeat_at TEXT NULL", []);

    // council_sessions columns added over time
    let _ = conn.execute("ALTER TABLE council_sessions ADD COLUMN session_summary TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE council_sessions ADD COLUMN updated_at TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE council_sessions ADD COLUMN conversation_id TEXT NULL", []);

    // council_messages columns added over time
    let _ = conn.execute("ALTER TABLE council_messages ADD COLUMN created_at TEXT NULL", []);

    // evidence_refs columns added over time
    let _ = conn.execute("ALTER TABLE evidence_refs ADD COLUMN related_session_id TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE evidence_refs ADD COLUMN related_memory_query TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE evidence_refs ADD COLUMN related_symbol TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE evidence_refs ADD COLUMN related_file TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE evidence_refs ADD COLUMN schema_version TEXT NULL", []);

    // task_events columns added over time
    let _ = conn.execute("ALTER TABLE task_events ADD COLUMN execution_action TEXT NULL", []);
    let _ = conn.execute("ALTER TABLE task_events ADD COLUMN execution_duration_seconds INTEGER NULL", []);

    // agent_heartbeat_events columns added over time
    let _ = conn.execute("ALTER TABLE agent_heartbeat_events ADD COLUMN related_task_id TEXT NULL", []);

    // Now run the full baseline schema to create any tables added after the initial install
    // (all IF NOT EXISTS — safe). Runs after column patches so indexes on new columns succeed.
    conn.execute_batch(BASE_SCHEMA)?;

    // Stamp migration version so to_latest() skips M0.
    conn.execute_batch("PRAGMA user_version = 1;")?;
    Ok(())
}

pub fn migrate_schema(conn: &mut Connection) -> StoreResult<()> {
    bootstrap_existing_db(conn)?;
    migrations()
        .to_latest(conn)
        .map_err(|e| super::StoreError::Validation(e.to_string()))?;
    Ok(())
}
