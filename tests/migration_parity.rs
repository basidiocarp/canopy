/// Schema parity test: fresh install vs bootstrap migration.
///
/// A fresh canopy install (empty DB → `migrate_schema`) and a bootstrapped
/// legacy install (pre-migration-framework DB → `migrate_schema`) can silently
/// diverge if a column is added to `BASE_SCHEMA` but not to the bootstrap
/// column-patch functions, or vice versa. This test catches that drift.
use canopy::store::Store;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use tempfile::tempdir;

// Minimal seed for a DB that was already at user_version=1 when the handoff
// and agent patch columns were introduced. Simulates an operator DB that
// bootstrap_existing_db did NOT patch (because user_version != 0) and that
// predates the new M7 back-patch migration.
//
// Tables are created with only the columns that existed before the add_*_columns
// patches were added (later columns are intentionally absent so M7 has real work
// to do). The seed then stamps user_version=1 so that bootstrap_existing_db
// early-returns and to_latest() runs M1..M7 — M1 creates review_annotations from
// scratch (not present in this seed), M2/M3 add tasks columns, M4..M7 are hooks.
const LEGACY_V1_SEED_SQL: &str = r"
    CREATE TABLE tasks (
        task_id           TEXT PRIMARY KEY,
        title             TEXT NOT NULL,
        description       TEXT NULL,
        requested_by      TEXT NOT NULL,
        project_root      TEXT NOT NULL,
        status            TEXT NOT NULL,
        verification_state TEXT NOT NULL,
        owner_agent_id    TEXT NULL
    );
    CREATE TABLE agents (
        agent_id       TEXT PRIMARY KEY,
        host_id        TEXT NOT NULL,
        host_type      TEXT NOT NULL,
        host_instance  TEXT NOT NULL,
        model          TEXT NOT NULL,
        project_root   TEXT NOT NULL,
        worktree_id    TEXT NOT NULL,
        status         TEXT NOT NULL,
        heartbeat_at   TEXT NULL
    );
    CREATE TABLE handoffs (
        handoff_id     TEXT PRIMARY KEY,
        task_id        TEXT NOT NULL,
        from_agent_id  TEXT NOT NULL,
        to_agent_id    TEXT NOT NULL,
        handoff_type   TEXT NOT NULL,
        summary        TEXT NOT NULL,
        requested_action TEXT NULL,
        status         TEXT NOT NULL
    );
    CREATE TABLE council_sessions (
        council_session_id TEXT PRIMARY KEY,
        task_id            TEXT NOT NULL,
        project_root       TEXT NOT NULL,
        worktree_id        TEXT NULL,
        participants_json  TEXT NOT NULL DEFAULT '[]',
        state              TEXT NOT NULL,
        transcript_ref     TEXT NULL,
        timeline_ref       TEXT NOT NULL,
        opened_at          TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    CREATE TABLE council_messages (
        message_id      TEXT PRIMARY KEY,
        task_id         TEXT NOT NULL,
        author_agent_id TEXT NOT NULL,
        message_type    TEXT NOT NULL,
        body            TEXT NOT NULL
    );
    CREATE TABLE evidence_refs (
        evidence_id  TEXT PRIMARY KEY,
        task_id      TEXT NOT NULL,
        source_kind  TEXT NOT NULL,
        source_ref   TEXT NOT NULL,
        label        TEXT NOT NULL,
        summary      TEXT NULL
    );
    CREATE TABLE task_events (
        event_id     TEXT PRIMARY KEY,
        task_id      TEXT NOT NULL,
        event_type   TEXT NOT NULL,
        actor        TEXT NOT NULL,
        from_status  TEXT NULL,
        to_status    TEXT NOT NULL,
        note         TEXT NULL,
        created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    CREATE TABLE agent_heartbeat_events (
        heartbeat_id    TEXT PRIMARY KEY,
        agent_id        TEXT NOT NULL,
        status          TEXT NOT NULL,
        current_task_id TEXT NULL,
        source          TEXT NOT NULL,
        created_at      TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    PRAGMA user_version = 1;
";

// Minimal seed representing a pre-migration-framework canopy install.
//
// bootstrap_existing_db (schema.rs:441) triggers when: tasks table exists AND
// user_version == 0. It applies ALTER TABLE column patches, then runs
// BASE_SCHEMA with IF NOT EXISTS, then stamps user_version=1. This seed
// replicates the minimal original schema so both migration paths can be
// compared from the same starting premise.
const LEGACY_SEED_SQL: &str = r"
    CREATE TABLE tasks (
        task_id           TEXT PRIMARY KEY,
        title             TEXT NOT NULL,
        description       TEXT NULL,
        requested_by      TEXT NOT NULL,
        project_root      TEXT NOT NULL,
        workspace         TEXT NULL,
        status            TEXT NOT NULL,
        verification_state TEXT NOT NULL,
        owner_agent_id    TEXT NULL,
        blocked_reason    TEXT NULL,
        verified_by       TEXT NULL,
        verified_at       TEXT NULL,
        closed_by         TEXT NULL,
        closure_summary   TEXT NULL,
        closed_at         TEXT NULL
    );
";

struct DbSchema {
    tables: HashSet<String>,
    indices: HashSet<String>,
    triggers: HashSet<String>,
    columns: HashMap<String, HashSet<String>>,
}

fn read_schema(path: &std::path::Path) -> DbSchema {
    let conn = Connection::open(path).expect("schema query connection");

    let mut tables = HashSet::new();
    let mut indices = HashSet::new();
    let mut triggers = HashSet::new();

    let mut stmt = conn
        .prepare(
            "SELECT type, name FROM sqlite_master
             WHERE name NOT LIKE 'sqlite_%'
             ORDER BY type, name",
        )
        .expect("prepare sqlite_master query");

    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query sqlite_master")
        .map(|r| r.expect("row"))
        .collect();

    for (obj_type, name) in rows {
        match obj_type.as_str() {
            "table" => {
                tables.insert(name);
            }
            "index" => {
                indices.insert(name);
            }
            "trigger" => {
                triggers.insert(name);
            }
            _ => {}
        }
    }

    let mut columns = HashMap::new();
    for table in &tables {
        let mut col_stmt = conn
            .prepare(&format!("PRAGMA table_info('{table}')"))
            .expect("prepare PRAGMA table_info");
        let cols: HashSet<String> = col_stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query table_info")
            .map(|r| r.expect("column name"))
            .collect();
        columns.insert(table.clone(), cols);
    }

    DbSchema {
        tables,
        indices,
        triggers,
        columns,
    }
}

#[test]
fn fresh_install_and_bootstrap_migration_produce_same_schema() {
    let fresh_dir = tempdir().expect("fresh tempdir");
    let fresh_path = fresh_dir.path().join("fresh.db");

    let legacy_dir = tempdir().expect("legacy tempdir");
    let legacy_path = legacy_dir.path().join("legacy.db");

    // Seed the legacy DB: tasks table present, user_version=0 → triggers bootstrap path.
    {
        let conn = Connection::open(&legacy_path).expect("seed connection");
        conn.execute_batch(LEGACY_SEED_SQL).expect("seed schema");
    }

    Store::open(&fresh_path).expect("fresh Store::open failed");
    Store::open(&legacy_path).expect("legacy Store::open failed");

    let fresh = read_schema(&fresh_path);
    let legacy = read_schema(&legacy_path);

    // Table parity.
    let fresh_only_tables: Vec<_> = fresh.tables.difference(&legacy.tables).collect();
    let legacy_only_tables: Vec<_> = legacy.tables.difference(&fresh.tables).collect();
    assert!(
        fresh_only_tables.is_empty() && legacy_only_tables.is_empty(),
        "Table set mismatch after migration.\n  Fresh only: {:?}\n  Legacy only: {:?}",
        fresh_only_tables,
        legacy_only_tables,
    );

    // Index parity.
    let fresh_only_idx: Vec<_> = fresh.indices.difference(&legacy.indices).collect();
    let legacy_only_idx: Vec<_> = legacy.indices.difference(&fresh.indices).collect();
    assert!(
        fresh_only_idx.is_empty() && legacy_only_idx.is_empty(),
        "Index set mismatch after migration.\n  Fresh only: {:?}\n  Legacy only: {:?}",
        fresh_only_idx,
        legacy_only_idx,
    );

    // Trigger parity.
    let fresh_only_trg: Vec<_> = fresh.triggers.difference(&legacy.triggers).collect();
    let legacy_only_trg: Vec<_> = legacy.triggers.difference(&fresh.triggers).collect();
    assert!(
        fresh_only_trg.is_empty() && legacy_only_trg.is_empty(),
        "Trigger set mismatch after migration.\n  Fresh only: {:?}\n  Legacy only: {:?}",
        fresh_only_trg,
        legacy_only_trg,
    );

    // Column parity per table (set comparison — column order may differ between paths).
    for table in &fresh.tables {
        let fresh_cols = fresh.columns.get(table).expect("fresh columns for table");
        let legacy_cols = legacy.columns.get(table).expect("legacy columns for table");
        let fresh_only_cols: Vec<_> = fresh_cols.difference(legacy_cols).collect();
        let legacy_only_cols: Vec<_> = legacy_cols.difference(fresh_cols).collect();
        assert!(
            fresh_only_cols.is_empty() && legacy_only_cols.is_empty(),
            "Column mismatch in table '{table}'.\n  Fresh only: {:?}\n  Legacy only: {:?}",
            fresh_only_cols,
            legacy_only_cols,
        );
    }
}

/// Regression test: M7 back-patches handoff and agent columns onto a DB that
/// was already at `user_version=1` when those columns were introduced.
///
/// `bootstrap_existing_db` only patches DBs at `user_version==0`. A DB stamped >=1
/// before the patch columns were added never received them, causing `canopy api task`
/// to fail. M7 closes this gap by re-running all add_*_columns patches as a safe
/// no-op for up-to-date DBs and a real repair for stale ones.
#[test]
fn migration_m7_back_patches_columns_on_user_version_1_db() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("v1legacy.db");

    // Seed the DB with pre-patch tables and stamp user_version=1 so that
    // bootstrap_existing_db returns early without patching.
    {
        let conn = Connection::open(&path).expect("seed connection");
        conn.execute_batch(LEGACY_V1_SEED_SQL).expect("seed schema");
    }

    // Confirm the columns are absent before migration.
    {
        let conn = Connection::open(&path).expect("pre-check connection");
        let handoff_cols_before: HashSet<String> = conn
            .prepare("PRAGMA table_info('handoffs')")
            .expect("prepare")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query")
            .map(|r| r.expect("col name"))
            .collect();
        assert!(
            !handoff_cols_before.contains("assignee_type"),
            "assignee_type should be absent before migration"
        );
    }

    // Run the full migration (bootstrap returns early; to_latest runs M1..M7).
    Store::open(&path).expect("Store::open failed on v1 legacy DB");

    let schema = read_schema(&path);

    // Handoff columns added by add_handoffs_columns must now be present.
    let handoff_cols = schema
        .columns
        .get("handoffs")
        .expect("handoffs table not found");
    for col in &[
        "assignee_type",
        "assignee_id",
        "disposition",
        "disposition_reason",
    ] {
        assert!(
            handoff_cols.contains(*col),
            "handoffs.{col} missing after M7 back-patch"
        );
    }

    // At least one column from add_agents_columns must also be present, confirming
    // the full patch suite ran and not just the handoffs patch.
    let agent_cols = schema
        .columns
        .get("agents")
        .expect("agents table not found");
    assert!(
        agent_cols.contains("last_heartbeat_at"),
        "agents.last_heartbeat_at missing after M7 back-patch"
    );
}
