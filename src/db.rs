use anyhow::{Context, Result};
use canopy::store::Store;
use std::path::{Path, PathBuf};

const CANOPY_DB_FILENAME: &str = "canopy.db";
const CANOPY_DB_ENV_VAR: &str = "CANOPY_DB_PATH";

pub fn open(db: Option<&Path>) -> Result<Store> {
    let path = resolve_db_path(db)?;
    Store::open(&path).context("open canopy store")
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
    use super::migrate_legacy_db_if_needed;
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
}
