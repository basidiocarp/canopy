use chrono::Utc;
use rusqlite::{OptionalExtension, params};

use super::helpers::{
    get_agent_in_connection, map_agent, map_agent_heartbeat, parse_database_timestamp,
    record_agent_heartbeat_in_connection, serialize_capabilities,
    sync_task_workflow_by_id_in_connection,
    validate_agent_registration, validate_agent_task_link,
};
use super::{AgentHeartbeatWrite, Store, StoreError, StoreResult};
use crate::models::{AgentHeartbeatEvent, AgentHeartbeatSource, AgentRegistration, AgentStatus};

impl Store {
    /// Registers or refreshes an agent entry in the local registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying database write fails.
    pub fn register_agent(&self, agent: &AgentRegistration) -> StoreResult<AgentRegistration> {
        self.in_transaction(|conn| {
            validate_agent_registration(conn, agent)?;
            conn.execute(
                r"
                INSERT INTO agents (
                    agent_id, host_id, host_type, host_instance, model,
                    project_root, worktree_id, role, capabilities, tier, specializations, status, current_task_id, heartbeat_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, CURRENT_TIMESTAMP)
                ON CONFLICT(agent_id) DO UPDATE SET
                    host_id = excluded.host_id,
                    host_type = excluded.host_type,
                    host_instance = excluded.host_instance,
                    model = excluded.model,
                    project_root = excluded.project_root,
                    worktree_id = excluded.worktree_id,
                    role = excluded.role,
                    capabilities = excluded.capabilities,
                    tier = excluded.tier,
                    specializations = excluded.specializations,
                    status = excluded.status,
                    current_task_id = excluded.current_task_id,
                    heartbeat_at = CURRENT_TIMESTAMP
                ",
                params![
                    agent.agent_id,
                    agent.host_id,
                    agent.host_type,
                    agent.host_instance,
                    agent.model,
                    agent.project_root,
                    agent.worktree_id,
                    agent.role.map(|value| value.to_string()),
                    serialize_capabilities(&agent.capabilities)?,
                    agent.tier.map(|value| value.to_string()),
                    serialize_capabilities(&agent.specializations)?,
                    agent.status.to_string(),
                    agent.current_task_id,
                ],
            )?;
            record_agent_heartbeat_in_connection(
                conn,
                &AgentHeartbeatWrite {
                    agent_id: &agent.agent_id,
                    status: agent.status,
                    current_task_id: agent.current_task_id.as_deref(),
                    related_task_id: agent.current_task_id.as_deref(),
                    source: AgentHeartbeatSource::Register,
                },
            )?;
            if let Some(task_id) = agent.current_task_id.as_deref() {
                sync_task_workflow_by_id_in_connection(conn, task_id)?;
            }
            get_agent_in_connection(conn, &agent.agent_id)
        })
    }

    /// Lists the registered agents in stable identifier order.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_agents(&self) -> StoreResult<Vec<AgentRegistration>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT agent_id, host_id, host_type, host_instance, model,
                   project_root, worktree_id, role, capabilities, tier, specializations, status, current_task_id, heartbeat_at
            FROM agents
            ORDER BY agent_id
            ",
        )?;
        let rows = stmt.query_map([], map_agent)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Updates an agent heartbeat and optional active-task context.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent does not exist or the update fails.
    pub fn heartbeat_agent(
        &self,
        agent_id: &str,
        status: AgentStatus,
        current_task_id: Option<&str>,
    ) -> StoreResult<AgentRegistration> {
        self.heartbeat_agent_with_workspace(agent_id, status, current_task_id, None)
    }

    /// Send a heartbeat with optional workspace (`project_root`) update.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent does not exist, the task link is invalid, or the write fails.
    pub fn heartbeat_agent_with_workspace(
        &self,
        agent_id: &str,
        status: AgentStatus,
        current_task_id: Option<&str>,
        workspace: Option<&str>,
    ) -> StoreResult<AgentRegistration> {
        self.ensure_agent_exists(agent_id)?;
        self.in_transaction(|conn| {
            validate_agent_task_link(conn, agent_id, status, current_task_id)?;

            if let Some(ws) = workspace {
                conn.execute(
                    r"
                    UPDATE agents
                    SET status = ?2,
                        current_task_id = ?3,
                        project_root = ?4,
                        heartbeat_at = CURRENT_TIMESTAMP
                    WHERE agent_id = ?1
                    ",
                    params![agent_id, status.to_string(), current_task_id, ws],
                )?;
            } else {
                conn.execute(
                    r"
                    UPDATE agents
                    SET status = ?2,
                        current_task_id = ?3,
                        heartbeat_at = CURRENT_TIMESTAMP
                    WHERE agent_id = ?1
                    ",
                    params![agent_id, status.to_string(), current_task_id],
                )?;
            }

            record_agent_heartbeat_in_connection(
                conn,
                &AgentHeartbeatWrite {
                    agent_id,
                    status,
                    current_task_id,
                    related_task_id: current_task_id,
                    source: AgentHeartbeatSource::Heartbeat,
                },
            )?;
            if let Some(task_id) = current_task_id {
                sync_task_workflow_by_id_in_connection(conn, task_id)?;
            }
            get_agent_in_connection(conn, agent_id)
        })
    }

    /// Loads a single agent by id.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent does not exist or the query fails.
    pub fn get_agent(&self, agent_id: &str) -> StoreResult<AgentRegistration> {
        get_agent_in_connection(&self.conn, agent_id)
    }

    /// Returns the age in seconds of the agent's last heartbeat.
    ///
    /// `None` means the agent has no recorded heartbeat yet.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent does not exist or the heartbeat timestamp is invalid.
    pub fn agent_last_heartbeat_age_secs(&self, agent_id: &str) -> StoreResult<Option<i64>> {
        let agent = self.get_agent(agent_id)?;
        let Some(heartbeat_at) = agent.heartbeat_at.as_deref() else {
            return Ok(None);
        };

        let heartbeat_at = parse_database_timestamp(heartbeat_at)?;
        let age_secs = (Utc::now() - heartbeat_at).num_seconds().max(0);
        Ok(Some(age_secs))
    }

    /// Ensure an agent's last heartbeat is fresh enough to claim work.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the agent is stale or missing a heartbeat.
    pub fn ensure_agent_fresh_for_claim(
        &self,
        agent_id: &str,
        threshold_secs: i64,
    ) -> StoreResult<()> {
        let age_secs = self.agent_last_heartbeat_age_secs(agent_id)?;
        match age_secs {
            Some(age_secs) if age_secs > threshold_secs => Err(StoreError::Validation(format!(
                "agent {agent_id} last heartbeat was {age_secs}s ago (threshold: {threshold_secs}s) — send a heartbeat before claiming"
            ))),
            Some(_) => Ok(()),
            None => Err(StoreError::Validation(format!(
                "agent {agent_id} has no recorded heartbeat (age: missing, threshold: {threshold_secs}s) — send a heartbeat before claiming"
            ))),
        }
    }

    /// Lists agents filtered by project root.
    ///
    /// When `project_root` is `None`, all agents are returned (equivalent to
    /// [`list_agents`](Self::list_agents)).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_agents_filtered(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<AgentRegistration>> {
        if let Some(project_root) = project_root {
            let mut stmt = self.conn.prepare(
                r"
                SELECT agent_id, host_id, host_type, host_instance, model,
                       project_root, worktree_id, role, capabilities, tier, specializations, status, current_task_id, heartbeat_at
                FROM agents
                WHERE project_root = ?1
                ORDER BY agent_id
                ",
            )?;
            let rows = stmt.query_map([project_root], map_agent)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::from)
        } else {
            self.list_agents()
        }
    }

    /// List agents whose last heartbeat is older than threshold.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_stale_agents(
        &self,
        stale_threshold_secs: i64,
    ) -> StoreResult<Vec<AgentRegistration>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT agent_id, host_id, host_type, host_instance, model,
                   project_root, worktree_id, role, capabilities, tier, specializations, status, current_task_id, heartbeat_at
            FROM agents
            WHERE heartbeat_at IS NOT NULL
              AND (julianday('now') - julianday(heartbeat_at)) * 86400 > ?1
            ORDER BY agent_id
            ",
        )?;
        let rows = stmt.query_map([stale_threshold_secs], map_agent)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// List agents with recent heartbeats (not stale).
    /// Uses a default threshold of 300 seconds (5 minutes).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_active_agents(&self) -> StoreResult<Vec<AgentRegistration>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT agent_id, host_id, host_type, host_instance, model,
                   project_root, worktree_id, role, capabilities, tier, specializations, status, current_task_id, heartbeat_at
            FROM agents
            WHERE heartbeat_at IS NOT NULL
              AND (julianday('now') - julianday(heartbeat_at)) * 86400 <= 300
            ORDER BY agent_id
            ",
        )?;
        let rows = stmt.query_map([], map_agent)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists heartbeat events, optionally filtered by agent or task.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_agent_heartbeats(
        &self,
        agent_id: Option<&str>,
        task_id: Option<&str>,
        limit: usize,
    ) -> StoreResult<Vec<AgentHeartbeatEvent>> {
        let limit = limit.max(1);
        let limit_i64 = i64::try_from(limit).map_err(|_| {
            StoreError::Validation("heartbeat limit exceeds supported range".to_string())
        })?;

        let mut heartbeats = Vec::new();
        match (agent_id, task_id) {
            (Some(agent_id), Some(task_id)) => {
                let mut stmt = self.conn.prepare(
                    r"
                    SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
                    FROM agent_heartbeat_events
                    WHERE agent_id = ?1 AND (current_task_id = ?2 OR related_task_id = ?2)
                    ORDER BY rowid DESC
                    LIMIT ?3
                    ",
                )?;
                let rows =
                    stmt.query_map(params![agent_id, task_id, limit_i64], map_agent_heartbeat)?;
                for row in rows {
                    heartbeats.push(row?);
                }
            }
            (Some(agent_id), None) => {
                let mut stmt = self.conn.prepare(
                    r"
                    SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
                    FROM agent_heartbeat_events
                    WHERE agent_id = ?1
                    ORDER BY rowid DESC
                    LIMIT ?2
                    ",
                )?;
                let rows = stmt.query_map(params![agent_id, limit_i64], map_agent_heartbeat)?;
                for row in rows {
                    heartbeats.push(row?);
                }
            }
            (None, Some(task_id)) => {
                let mut stmt = self.conn.prepare(
                    r"
                    SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
                    FROM agent_heartbeat_events
                    WHERE current_task_id = ?1 OR related_task_id = ?1
                    ORDER BY rowid DESC
                    LIMIT ?2
                    ",
                )?;
                let rows = stmt.query_map(params![task_id, limit_i64], map_agent_heartbeat)?;
                for row in rows {
                    heartbeats.push(row?);
                }
            }
            (None, None) => {
                let mut stmt = self.conn.prepare(
                    r"
                    SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
                    FROM agent_heartbeat_events
                    ORDER BY rowid DESC
                    LIMIT ?1
                    ",
                )?;
                let rows = stmt.query_map(params![limit_i64], map_agent_heartbeat)?;
                for row in rows {
                    heartbeats.push(row?);
                }
            }
        }

        Ok(heartbeats)
    }

    /// Lists all heartbeat events without pre-filter truncation.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_all_agent_heartbeats(&self) -> StoreResult<Vec<AgentHeartbeatEvent>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
            FROM agent_heartbeat_events
            ORDER BY rowid DESC
            ",
        )?;
        let rows = stmt.query_map([], map_agent_heartbeat)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists heartbeat history relevant to a task, including stop/idle events.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn list_task_heartbeats(
        &self,
        task_id: &str,
        limit: usize,
    ) -> StoreResult<Vec<AgentHeartbeatEvent>> {
        let task = self.get_task(task_id)?;
        let limit_i64 = i64::try_from(limit.max(1)).map_err(|_| {
            StoreError::Validation("heartbeat limit exceeds supported range".to_string())
        })?;

        let mut stmt = self.conn.prepare(
            r"
            WITH related_agents AS (
                SELECT owner_agent_id AS agent_id
                FROM task_events
                WHERE task_id = ?1 AND owner_agent_id IS NOT NULL
                UNION
                SELECT from_agent_id AS agent_id
                FROM handoffs
                WHERE task_id = ?1
                UNION
                SELECT to_agent_id AS agent_id
                FROM handoffs
                WHERE task_id = ?1
                UNION
                SELECT owner_agent_id AS agent_id
                FROM tasks
                WHERE task_id = ?1 AND owner_agent_id IS NOT NULL
            )
            SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
            FROM agent_heartbeat_events
            WHERE agent_id IN (SELECT agent_id FROM related_agents)
              AND created_at >= ?2
              AND (current_task_id = ?1 OR related_task_id = ?1)
            ORDER BY rowid DESC
            LIMIT ?3
            ",
        )?;
        let rows = stmt.query_map(
            params![task_id, task.created_at, limit_i64],
            map_agent_heartbeat,
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// Lists recent agent heartbeat events for agents in a project, with a row limit.
    ///
    /// When `project_root` is `None`, all heartbeats are returned (up to the
    /// specified limit). `limit` defaults to 50 when `None`.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_agent_heartbeats_for_project(
        &self,
        project_root: Option<&str>,
        limit: Option<i64>,
    ) -> StoreResult<Vec<AgentHeartbeatEvent>> {
        let lim = limit.unwrap_or(50);
        if let Some(project_root) = project_root {
            let mut stmt = self.conn.prepare(
                r"
                SELECT h.heartbeat_id, h.agent_id, h.status, h.current_task_id, h.related_task_id, h.source, h.created_at
                FROM agent_heartbeat_events h
                JOIN agents a ON a.agent_id = h.agent_id
                WHERE a.project_root = ?1
                ORDER BY h.rowid DESC
                LIMIT ?2
                ",
            )?;
            let rows = stmt.query_map(params![project_root, lim], map_agent_heartbeat)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::from)
        } else {
            let mut stmt = self.conn.prepare(
                r"
                SELECT heartbeat_id, agent_id, status, current_task_id, related_task_id, source, created_at
                FROM agent_heartbeat_events
                ORDER BY rowid DESC
                LIMIT ?1
                ",
            )?;
            let rows = stmt.query_map([lim], map_agent_heartbeat)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::from)
        }
    }

    pub(crate) fn ensure_agent_exists(&self, agent_id: &str) -> StoreResult<()> {
        let exists = self
            .conn
            .query_row(
                "SELECT 1 FROM agents WHERE agent_id = ?1",
                [agent_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        exists.ok_or(StoreError::NotFound("agent"))?;
        Ok(())
    }

    /// Rank agents for task assignment based on capability match and tier.
    ///
    /// Scoring logic:
    /// - Base score: 50.0
    /// - If task has `requires_planning` tag and agent tier is Planner: +30.0
    /// - If task has `requires_planning` tag and agent tier is Grunt: -30.0
    /// - Per matching specialization in `required_tags`: +10.0
    /// - Active agents (heartbeat in last 5 minutes): +20.0
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn rank_agents_for_task(
        &self,
        task_id: &str,
        required_tags: &[String],
    ) -> StoreResult<Vec<crate::models::AgentRankEntry>> {
        let agents = self.list_active_agents().unwrap_or_default();
        let has_planning_tag = required_tags.iter().any(|tag| tag == "requires_planning");

        // Fetch task to check for workspace affinity
        let task_workspace = self.get_task(task_id).ok().and_then(|task| task.workspace);

        let mut ranked: Vec<_> = agents
            .into_iter()
            .map(|agent| {
                let mut score = 50.0_f32;
                let mut reasons = Vec::new();

                // Tier-based scoring for planning tasks
                if has_planning_tag {
                    match agent.tier {
                        Some(crate::models::AgentTier::Planner) => {
                            score += 30.0;
                            reasons.push("matches_planning_tier".to_string());
                        }
                        Some(crate::models::AgentTier::Grunt) => {
                            score -= 30.0;
                            reasons.push("grunt_not_suitable_for_planning".to_string());
                        }
                        Some(crate::models::AgentTier::Worker) => {
                            reasons.push("worker_tier".to_string());
                        }
                        None => {
                            reasons.push("tier_unspecified".to_string());
                        }
                    }
                }

                // Specialization matching
                let specialization_matches = agent
                    .specializations
                    .iter()
                    .filter(|spec| required_tags.contains(spec))
                    .count();
                #[allow(clippy::cast_precision_loss)]
                {
                    score += (specialization_matches as f32) * 10.0;
                }
                if specialization_matches > 0 {
                    reasons.push(format!("{specialization_matches}_specializations_match"));
                }

                // Active status bonus
                if agent.heartbeat_at.is_some() {
                    score += 20.0;
                    reasons.push("recently_active".to_string());
                }

                // Workspace affinity scoring
                if let Some(ref workspace) = task_workspace {
                    if agent.project_root == *workspace {
                        score += 40.0;
                        reasons.push("workspace_affinity_match".to_string());
                    } else {
                        tracing::warn!(
                            "workspace affinity mismatch: agent {} in '{}', task requires '{}'",
                            agent.agent_id,
                            agent.project_root,
                            workspace
                        );
                    }
                }

                crate::models::AgentRankEntry {
                    agent_id: agent.agent_id,
                    score,
                    reasons,
                }
            })
            .collect();

        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.agent_id.cmp(&b.agent_id))
        });

        Ok(ranked)
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{AgentRegistration, AgentStatus};
    use crate::store::{Store, TaskCreationOptions};
    use tempfile::tempdir;

    fn test_store() -> Store {
        let dir = tempdir().expect("temp dir");
        Store::open(&dir.path().join("canopy.db")).expect("store")
    }

    fn register_agent_at(store: &Store, agent_id: &str, project_root: &str) {
        store
            .register_agent(&AgentRegistration {
                agent_id: agent_id.to_string(),
                host_id: "host-1".to_string(),
                host_type: "claude".to_string(),
                host_instance: "local".to_string(),
                model: "claude-sonnet".to_string(),
                project_root: project_root.to_string(),
                worktree_id: "main".to_string(),
                role: None,
                capabilities: Vec::new(),
                tier: None,
                specializations: Vec::new(),
                status: AgentStatus::Idle,
                current_task_id: None,
                heartbeat_at: None,
            })
            .expect("register_agent");
    }

    fn task_for_workspace(store: &Store, workspace: &str) -> String {
        store
            .create_task_with_options(
                "test task",
                None,
                "operator",
                workspace,
                &TaskCreationOptions {
                    workspace: Some(workspace.to_string()),
                    ..TaskCreationOptions::default()
                },
            )
            .expect("create_task")
            .task_id
    }

    #[test]
    fn workspace_match_scores_higher_than_mismatch() {
        let store = test_store();
        register_agent_at(&store, "agent-local", "/workspace/alpha");
        register_agent_at(&store, "agent-remote", "/workspace/beta");

        let task_id = task_for_workspace(&store, "/workspace/alpha");
        let ranked = store.rank_agents_for_task(&task_id, &[]).expect("rank");

        let local = ranked.iter().find(|r| r.agent_id == "agent-local").unwrap();
        let remote = ranked
            .iter()
            .find(|r| r.agent_id == "agent-remote")
            .unwrap();
        assert!(
            local.score > remote.score,
            "workspace-matching agent (score={}) should outscore mismatched (score={})",
            local.score,
            remote.score
        );
        assert!(
            local
                .reasons
                .contains(&"workspace_affinity_match".to_string()),
            "matching agent should carry workspace_affinity_match reason"
        );
    }

    #[test]
    fn workspace_match_bonus_is_forty_points() {
        let store = test_store();
        register_agent_at(&store, "agent-match", "/workspace/proj");
        register_agent_at(&store, "agent-miss", "/workspace/other");

        let task_id = task_for_workspace(&store, "/workspace/proj");
        let ranked = store.rank_agents_for_task(&task_id, &[]).expect("rank");

        let matched = ranked.iter().find(|r| r.agent_id == "agent-match").unwrap();
        let missed = ranked.iter().find(|r| r.agent_id == "agent-miss").unwrap();
        assert!(
            (matched.score - missed.score - 40.0).abs() < 0.001,
            "workspace affinity bonus must be exactly 40 points, got {}",
            matched.score - missed.score
        );
    }

    #[test]
    fn task_without_workspace_leaves_scores_equal() {
        let store = test_store();
        register_agent_at(&store, "agent-a", "/workspace/alpha");
        register_agent_at(&store, "agent-b", "/workspace/beta");

        let task_id = store
            .create_task_with_options(
                "no-workspace task",
                None,
                "operator",
                "/workspace/alpha",
                &TaskCreationOptions::default(),
            )
            .expect("create_task")
            .task_id;

        let ranked = store.rank_agents_for_task(&task_id, &[]).expect("rank");
        let scores: Vec<f32> = ranked.iter().map(|r| r.score).collect();
        assert!(
            scores
                .windows(2)
                .all(|w| (w[0] - w[1]).abs() < f32::EPSILON),
            "agents should tie when task has no workspace: {:?}",
            ranked
                .iter()
                .map(|r| (&r.agent_id, r.score))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn heartbeat_with_workspace_updates_project_root() {
        let store = test_store();
        register_agent_at(&store, "agent-ws", "/workspace/old");

        store
            .heartbeat_agent_with_workspace(
                "agent-ws",
                AgentStatus::Idle,
                None,
                Some("/workspace/new"),
            )
            .expect("heartbeat_with_workspace");

        let agent = store.get_agent("agent-ws").expect("get_agent");
        assert_eq!(agent.project_root, "/workspace/new");
    }

    #[test]
    fn heartbeat_without_workspace_preserves_project_root() {
        let store = test_store();
        register_agent_at(&store, "agent-keep", "/workspace/stable");

        store
            .heartbeat_agent("agent-keep", AgentStatus::Idle, None)
            .expect("heartbeat");

        let agent = store.get_agent("agent-keep").expect("get_agent");
        assert_eq!(agent.project_root, "/workspace/stable");
    }
}
