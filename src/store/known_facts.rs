use rusqlite::params;
use std::str::FromStr;

use super::{Store, StoreError, StoreResult};
use crate::models::{FactScope, FactType, KnownFact};

fn map_known_fact(row: &rusqlite::Row<'_>) -> rusqlite::Result<KnownFact> {
    let fact_type_str: String = row.get(2)?;
    let scope_str: String = row.get(3)?;
    Ok(KnownFact {
        fact_id: row.get(0)?,
        key: row.get(1)?,
        fact_type: FactType::from_str(&fact_type_str).unwrap_or(FactType::Other),
        scope: FactScope::from_str(&scope_str).unwrap_or(FactScope::Project),
        summary: row.get(4)?,
        hyphae_id: row.get(5)?,
        established_by: row.get(6)?,
        task_id: row.get(7)?,
        confidence: row.get(8)?,
        established_at: row.get(9)?,
    })
}

impl Store {
    /// Insert a known fact into the registry.
    ///
    /// If a fact with the same `key` and `scope` already exists, the existing
    /// record is returned unchanged. Use `upsert_known_fact` to overwrite.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_known_fact(
        &self,
        fact_id: &str,
        key: &str,
        fact_type: &FactType,
        scope: &FactScope,
        summary: &str,
        hyphae_id: Option<&str>,
        established_by: &str,
        task_id: Option<&str>,
        confidence: f32,
    ) -> StoreResult<KnownFact> {
        self.conn.execute(
            r"
            INSERT OR IGNORE INTO known_facts
                (fact_id, key, fact_type, scope, summary, hyphae_id,
                 established_by, task_id, confidence, established_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))
            ",
            params![
                fact_id,
                key,
                fact_type.to_string(),
                scope.to_string(),
                summary,
                hyphae_id,
                established_by,
                task_id,
                confidence,
            ],
        )?;
        self.get_known_fact_by_id(fact_id)
    }

    fn get_known_fact_by_id(&self, fact_id: &str) -> StoreResult<KnownFact> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT fact_id, key, fact_type, scope, summary, hyphae_id,
                   established_by, task_id, confidence, established_at
            FROM known_facts
            WHERE fact_id = ?1
            ",
        )?;
        stmt.query_row([fact_id], map_known_fact)
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound("known_fact"),
                other => StoreError::from(other),
            })
    }

    /// Query known facts by key, scope, and optional task filter.
    ///
    /// All parameters are optional; omitting them returns all facts ordered by
    /// `established_at` descending (newest first). When `keys` is non-empty,
    /// only facts whose key exactly matches one of the supplied strings are
    /// returned.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn query_known_facts(
        &self,
        keys: Option<&[String]>,
        scope: Option<&FactScope>,
        task_id: Option<&str>,
    ) -> StoreResult<Vec<KnownFact>> {
        let mut conditions: Vec<String> = Vec::new();
        let mut positional: Vec<String> = Vec::new();
        let mut idx = 1usize;

        if let Some(scope) = scope {
            conditions.push(format!("scope = ?{idx}"));
            positional.push(scope.to_string());
            idx += 1;
        }

        if let Some(task_id) = task_id {
            conditions.push(format!("task_id = ?{idx}"));
            positional.push(task_id.to_string());
            idx += 1;
        }

        let key_placeholders = keys.filter(|k| !k.is_empty()).map(|ks| {
            let placeholders: Vec<String> = ks
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", idx + i))
                .collect();
            let clause = format!("key IN ({})", placeholders.join(", "));
            positional.extend(ks.iter().cloned());
            clause
        });

        if let Some(clause) = key_placeholders {
            conditions.push(clause);
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            r"
            SELECT fact_id, key, fact_type, scope, summary, hyphae_id,
                   established_by, task_id, confidence, established_at
            FROM known_facts
            {where_clause}
            ORDER BY established_at DESC
            "
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = positional
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), map_known_fact)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn open() -> Store {
        Store::open(Path::new(":memory:")).expect("in-memory store")
    }

    #[test]
    fn insert_and_query_known_fact() {
        let store = open();
        let id = ulid::Ulid::new().to_string();
        let fact = store
            .insert_known_fact(
                &id,
                "arch/event-model",
                &FactType::Decision,
                &FactScope::Project,
                "Events are immutable once written",
                Some("hyp_abc123"),
                "agent-1",
                None,
                0.95,
            )
            .expect("insert");
        assert_eq!(fact.key, "arch/event-model");
        assert_eq!(fact.fact_type, FactType::Decision);

        let results = store
            .query_known_facts(Some(&["arch/event-model".to_string()]), None, None)
            .expect("query");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].hyphae_id.as_deref(), Some("hyp_abc123"));
    }

    #[test]
    fn insert_or_ignore_is_idempotent() {
        let store = open();
        let id = ulid::Ulid::new().to_string();
        store
            .insert_known_fact(
                &id,
                "my/key",
                &FactType::Constraint,
                &FactScope::Project,
                "first summary",
                None,
                "agent-1",
                None,
                1.0,
            )
            .expect("first insert");
        // Second call with the same id should not error; existing row is silently kept
        store
            .insert_known_fact(
                &id,
                "my/key",
                &FactType::Constraint,
                &FactScope::Project,
                "second summary (should not overwrite)",
                None,
                "agent-2",
                None,
                0.5,
            )
            .expect("second insert (no-op)");
        let results = store
            .query_known_facts(Some(&["my/key".to_string()]), None, None)
            .expect("query");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].summary, "first summary");
    }

    #[test]
    fn query_by_scope_filters_correctly() {
        let store = open();
        let id1 = ulid::Ulid::new().to_string();
        let id2 = ulid::Ulid::new().to_string();
        store
            .insert_known_fact(
                &id1,
                "proj/key",
                &FactType::Other,
                &FactScope::Project,
                "project-scoped",
                None,
                "agent-1",
                None,
                1.0,
            )
            .expect("insert1");
        store
            .insert_known_fact(
                &id2,
                "file/key",
                &FactType::Other,
                &FactScope::File,
                "file-scoped",
                None,
                "agent-1",
                None,
                1.0,
            )
            .expect("insert2");

        let project_only = store
            .query_known_facts(None, Some(&FactScope::Project), None)
            .expect("query project");
        assert!(project_only.iter().all(|f| f.scope == FactScope::Project));

        let all = store
            .query_known_facts(None, None, None)
            .expect("query all");
        assert_eq!(all.len(), 2);
    }
}
