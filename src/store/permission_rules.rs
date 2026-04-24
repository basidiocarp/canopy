use super::StoreResult;
use rusqlite::{Connection, OptionalExtension};

pub struct PermissionRule {
    pub rule_id: String,
    pub agent_id: String, // "*" for wildcard
    pub tool_name: String,
    pub action: String, // "allow" | "deny"
    pub scope: String,  // "session" | "permanent"
    pub reason: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
}

/// Look up a persisted permission rule for an agent and tool.
///
/// Returns the matching rule if found and not expired. Checks exact agent match first,
/// then falls back to wildcard agent "*". Returns `None` if no rule matches or all
/// matching rules have expired.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub fn lookup_rule(
    conn: &Connection,
    agent_id: &str,
    tool_name: &str,
    now_ms: i64,
) -> StoreResult<Option<PermissionRule>> {
    let mut stmt = conn.prepare(
        "SELECT rule_id, agent_id, tool_name, action, scope, reason, created_at, expires_at
         FROM permission_rules
         WHERE (agent_id = ?1 OR agent_id = '*')
           AND tool_name = ?2
           AND (expires_at IS NULL OR expires_at > ?3)
         -- ORDER BY agent_id DESC gives exact agent matches priority over the '*' wildcard.
         -- This works because '*' (0x2A) sorts below all printable alphanumeric characters
         -- and all ULID/UUID formats, so any specific agent_id ranks higher than '*' in
         -- descending order.
         ORDER BY agent_id DESC
         LIMIT 1",
    )?;

    let rule = stmt
        .query_row(rusqlite::params![agent_id, tool_name, now_ms], |row| {
            Ok(PermissionRule {
                rule_id: row.get(0)?,
                agent_id: row.get(1)?,
                tool_name: row.get(2)?,
                action: row.get(3)?,
                scope: row.get(4)?,
                reason: row.get(5)?,
                created_at: row.get(6)?,
                expires_at: row.get(7)?,
            })
        })
        .optional()?;

    Ok(rule)
}

/// Insert or replace a persisted permission rule.
///
/// # Errors
///
/// Returns an error if the database insert fails.
pub fn upsert_rule(conn: &Connection, rule: &PermissionRule) -> StoreResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO permission_rules
             (rule_id, agent_id, tool_name, action, scope, reason, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            rule.rule_id,
            rule.agent_id,
            rule.tool_name,
            rule.action,
            rule.scope,
            rule.reason,
            rule.created_at,
            rule.expires_at,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE permission_rules (
                rule_id     TEXT PRIMARY KEY,
                agent_id    TEXT NOT NULL,
                tool_name   TEXT NOT NULL,
                action      TEXT NOT NULL CHECK(action IN ('allow', 'deny')),
                scope       TEXT NOT NULL CHECK(scope IN ('session', 'permanent')),
                reason      TEXT NOT NULL DEFAULT '',
                created_at  INTEGER NOT NULL,
                expires_at  INTEGER
            );
            CREATE INDEX idx_permission_rules_lookup
                ON permission_rules(agent_id, tool_name);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn lookup_returns_none_when_no_rule() {
        let conn = setup();
        let result = lookup_rule(&conn, "agent-1", "canopy_task_list", 1_000_000).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn upsert_and_lookup_allow_rule() {
        let conn = setup();
        let rule = PermissionRule {
            rule_id: "rule-01".to_string(),
            agent_id: "agent-1".to_string(),
            tool_name: "canopy_task_list".to_string(),
            action: "allow".to_string(),
            scope: "permanent".to_string(),
            reason: "approved for task listing".to_string(),
            created_at: 1_000_000,
            expires_at: None,
        };

        upsert_rule(&conn, &rule).unwrap();
        let retrieved = lookup_rule(&conn, "agent-1", "canopy_task_list", 1_000_001)
            .unwrap()
            .expect("rule should be found");

        assert_eq!(retrieved.rule_id, "rule-01");
        assert_eq!(retrieved.action, "allow");
        assert_eq!(retrieved.scope, "permanent");
        assert_eq!(retrieved.reason, "approved for task listing");
    }

    #[test]
    fn wildcard_agent_matches_any() {
        let conn = setup();
        let wildcard_rule = PermissionRule {
            rule_id: "rule-wildcard".to_string(),
            agent_id: "*".to_string(),
            tool_name: "canopy_task_complete".to_string(),
            action: "deny".to_string(),
            scope: "permanent".to_string(),
            reason: "destructive tool blocked".to_string(),
            created_at: 1_000_000,
            expires_at: None,
        };

        upsert_rule(&conn, &wildcard_rule).unwrap();

        // Any agent should match the wildcard rule
        let retrieved = lookup_rule(&conn, "agent-xyz", "canopy_task_complete", 1_000_001)
            .unwrap()
            .expect("wildcard rule should match any agent");

        assert_eq!(retrieved.agent_id, "*");
        assert_eq!(retrieved.action, "deny");
    }

    #[test]
    fn expired_rule_not_returned() {
        let conn = setup();
        let rule = PermissionRule {
            rule_id: "rule-expired".to_string(),
            agent_id: "agent-1".to_string(),
            tool_name: "canopy_task_complete".to_string(),
            action: "allow".to_string(),
            scope: "session".to_string(),
            reason: "temporary override".to_string(),
            created_at: 1_000_000,
            expires_at: Some(2_000_000), // expires at this time
        };

        upsert_rule(&conn, &rule).unwrap();

        // Look up before expiration: should find it
        let before = lookup_rule(&conn, "agent-1", "canopy_task_complete", 1_500_000).unwrap();
        assert!(before.is_some());

        // Look up after expiration: should not find it
        let after = lookup_rule(&conn, "agent-1", "canopy_task_complete", 2_500_000).unwrap();
        assert!(after.is_none());
    }

    #[test]
    fn exact_agent_match_overrides_wildcard() {
        let conn = setup();
        // Wildcard deny for any agent
        upsert_rule(
            &conn,
            &PermissionRule {
                rule_id: "01HWILD".into(),
                agent_id: "*".into(),
                tool_name: "canopy_task_delete".into(),
                action: "deny".into(),
                scope: "permanent".into(),
                reason: "default deny".into(),
                created_at: 1_000_000,
                expires_at: None,
            },
        )
        .unwrap();
        // Specific allow for agent-1
        upsert_rule(
            &conn,
            &PermissionRule {
                rule_id: "01HEXACT".into(),
                agent_id: "agent-1".into(),
                tool_name: "canopy_task_delete".into(),
                action: "allow".into(),
                scope: "permanent".into(),
                reason: "operator approved".into(),
                created_at: 1_000_000,
                expires_at: None,
            },
        )
        .unwrap();
        // agent-1 should get the specific allow, not the wildcard deny
        let rule = lookup_rule(&conn, "agent-1", "canopy_task_delete", 1_000_001).unwrap();
        assert!(rule.is_some());
        assert_eq!(rule.unwrap().action, "allow");
    }
}
