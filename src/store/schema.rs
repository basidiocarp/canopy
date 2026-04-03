use rusqlite::Connection;

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
        heartbeat_at TEXT NULL
    );

    CREATE TABLE IF NOT EXISTS tasks (
        task_id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        description TEXT NULL,
        requested_by TEXT NOT NULL,
        project_root TEXT NOT NULL,
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
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
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
        body TEXT NOT NULL
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
        task_id TEXT NOT NULL REFERENCES tasks(task_id),
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
";

#[allow(clippy::too_many_lines)]
pub(crate) fn migrate_schema(conn: &Connection) -> StoreResult<()> {
    ensure_column(conn, "tasks", "priority", "TEXT NULL")?;
    ensure_column(conn, "tasks", "severity", "TEXT NULL")?;
    ensure_column(conn, "tasks", "required_role", "TEXT NULL")?;
    ensure_column(
        conn,
        "tasks",
        "required_capabilities",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(conn, "tasks", "auto_review", "INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(
        conn,
        "tasks",
        "verification_required",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(conn, "tasks", "owner_note", "TEXT NULL")?;
    ensure_column(conn, "tasks", "acknowledged_by", "TEXT NULL")?;
    ensure_column(conn, "tasks", "acknowledged_at", "TEXT NULL")?;
    ensure_column(conn, "tasks", "due_at", "TEXT NULL")?;
    ensure_column(conn, "tasks", "review_due_at", "TEXT NULL")?;
    ensure_column(conn, "tasks", "created_at", "TEXT NULL")?;
    ensure_column(conn, "tasks", "updated_at", "TEXT NULL")?;
    conn.execute(
        r"
        UPDATE tasks
        SET priority = COALESCE(priority, 'medium'),
            severity = COALESCE(severity, 'none'),
            required_capabilities = COALESCE(required_capabilities, '[]'),
            auto_review = COALESCE(auto_review, 0),
            verification_required = COALESCE(verification_required, 0),
            created_at = COALESCE(
                created_at,
                (SELECT MIN(created_at) FROM task_events WHERE task_events.task_id = tasks.task_id),
                CURRENT_TIMESTAMP
            ),
            updated_at = COALESCE(
                updated_at,
                (SELECT MAX(created_at) FROM task_events WHERE task_events.task_id = tasks.task_id),
                closed_at,
                verified_at,
                created_at,
                CURRENT_TIMESTAMP
            )
        ",
        [],
    )?;

    ensure_column(conn, "handoffs", "due_at", "TEXT NULL")?;
    ensure_column(conn, "handoffs", "expires_at", "TEXT NULL")?;
    ensure_column(conn, "handoffs", "created_at", "TEXT NULL")?;
    ensure_column(conn, "handoffs", "updated_at", "TEXT NULL")?;
    ensure_column(conn, "handoffs", "resolved_at", "TEXT NULL")?;
    conn.execute(
        r"
        UPDATE handoffs
        SET created_at = COALESCE(
                created_at,
                (SELECT created_at FROM tasks WHERE tasks.task_id = handoffs.task_id),
                CURRENT_TIMESTAMP
            ),
            updated_at = COALESCE(
                updated_at,
                resolved_at,
                (SELECT updated_at FROM tasks WHERE tasks.task_id = handoffs.task_id),
                created_at,
                CURRENT_TIMESTAMP
            )
        ",
        [],
    )?;

    ensure_column(conn, "evidence_refs", "related_session_id", "TEXT NULL")?;
    ensure_column(conn, "evidence_refs", "related_memory_query", "TEXT NULL")?;
    ensure_column(conn, "evidence_refs", "related_symbol", "TEXT NULL")?;
    ensure_column(conn, "evidence_refs", "related_file", "TEXT NULL")?;
    ensure_column(
        conn,
        "evidence_refs",
        "schema_version",
        "TEXT NOT NULL DEFAULT '1.0'",
    )?;
    conn.execute(
        "UPDATE evidence_refs SET schema_version = COALESCE(schema_version, '1.0')",
        [],
    )?;
    ensure_column(conn, "task_events", "execution_action", "TEXT NULL")?;
    ensure_column(
        conn,
        "task_events",
        "execution_duration_seconds",
        "INTEGER NULL",
    )?;
    ensure_column(
        conn,
        "agent_heartbeat_events",
        "related_task_id",
        "TEXT NULL",
    )?;
    ensure_column(conn, "agents", "role", "TEXT NULL")?;
    ensure_column(conn, "agents", "capabilities", "TEXT NOT NULL DEFAULT '[]'")?;
    conn.execute(
        "UPDATE agents SET capabilities = COALESCE(capabilities, '[]')",
        [],
    )?;
    conn.execute_batch(
        r"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_task_relationships_parent_source
        ON task_relationships(source_task_id)
        WHERE kind = 'parent'
        ",
    )?;

    // File-scope conflict detection
    ensure_column(conn, "tasks", "scope", "TEXT NOT NULL DEFAULT '[]'")?;

    // Track 1 (Foundation) columns
    ensure_column(conn, "tasks", "claimed_at", "TEXT NULL")?;
    ensure_column(conn, "tasks", "files_hint", "TEXT NULL")?;
    ensure_column(conn, "agents", "last_heartbeat_at", "TEXT NULL")?;

    // Ensure file_locks table and indexes exist for older databases
    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS file_locks (
            lock_id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(task_id),
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
        ",
    )?;

    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> StoreResult<()> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&pragma)?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;

    for existing in columns {
        if existing? == column {
            return Ok(());
        }
    }

    let alter = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
    conn.execute(&alter, [])?;
    Ok(())
}
