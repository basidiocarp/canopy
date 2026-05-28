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
