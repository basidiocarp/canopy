use super::helpers::{
    delete_task_relationship_in_connection, find_task_relationship_between_in_connection,
    get_task_in_connection, is_open_task_status, list_source_task_relationships_in_connection,
    map_task_relationship, record_task_event_in_connection,
};
use super::{Store, StoreError, StoreResult, TaskEventWrite};
use crate::models::{
    OperatorActionKind, RelatedTask, Task, TaskEventType, TaskRelationship,
    TaskRelationshipKind, TaskRelationshipRole,
};

impl Store {
    /// Lists task relationships globally or for one task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn list_task_relationships(
        &self,
        task_id: Option<&str>,
    ) -> StoreResult<Vec<TaskRelationship>> {
        let mut relationships = Vec::new();
        if let Some(task_id) = task_id {
            self.ensure_task_exists(task_id)?;
            let mut stmt = self.conn.prepare(
                r"
                SELECT relationship_id, source_task_id, target_task_id, kind, created_by, created_at, updated_at
                FROM task_relationships
                WHERE source_task_id = ?1 OR target_task_id = ?1
                ORDER BY rowid
                ",
            )?;
            let rows = stmt.query_map([task_id], map_task_relationship)?;
            for row in rows {
                relationships.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                r"
                SELECT relationship_id, source_task_id, target_task_id, kind, created_by, created_at, updated_at
                FROM task_relationships
                ORDER BY rowid
                ",
            )?;
            let rows = stmt.query_map([], map_task_relationship)?;
            for row in rows {
                relationships.push(row?);
            }
        }
        Ok(relationships)
    }

    /// Loads directional related-task summaries for one task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task does not exist or the query fails.
    pub fn list_related_tasks(&self, task_id: &str) -> StoreResult<Vec<RelatedTask>> {
        self.ensure_task_exists(task_id)?;
        let relationships = self.list_task_relationships(Some(task_id))?;
        relationships
            .into_iter()
            .map(|relationship| {
                let (related_task_id, relationship_role) = if relationship.source_task_id == task_id
                {
                    let role = match relationship.kind {
                        TaskRelationshipKind::FollowUp => TaskRelationshipRole::FollowUpChild,
                        TaskRelationshipKind::Blocks => TaskRelationshipRole::Blocks,
                        TaskRelationshipKind::Parent => TaskRelationshipRole::Parent,
                    };
                    (relationship.target_task_id.clone(), role)
                } else {
                    let role = match relationship.kind {
                        TaskRelationshipKind::FollowUp => TaskRelationshipRole::FollowUpParent,
                        TaskRelationshipKind::Blocks => TaskRelationshipRole::BlockedBy,
                        TaskRelationshipKind::Parent => TaskRelationshipRole::Child,
                    };
                    (relationship.source_task_id.clone(), role)
                };
                let related_task = self.get_task(&related_task_id)?;
                Ok(RelatedTask {
                    relationship_id: relationship.relationship_id,
                    relationship_kind: relationship.kind,
                    relationship_role,
                    related_task_id: related_task.task_id,
                    title: related_task.title,
                    status: related_task.status,
                    verification_state: related_task.verification_state,
                    priority: related_task.priority,
                    severity: related_task.severity,
                    owner_agent_id: related_task.owner_agent_id,
                    blocked_reason: related_task.blocked_reason,
                    created_at: related_task.created_at,
                    updated_at: related_task.updated_at,
                })
            })
            .collect()
    }

    /// Lists task relationships for all tasks in a project.
    ///
    /// Only relationships where both source and target tasks belong to the
    /// given project are returned. When `project_root` is `None`, all
    /// relationships are returned.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_task_relationships_for_project(
        &self,
        project_root: Option<&str>,
    ) -> StoreResult<Vec<TaskRelationship>> {
        if let Some(project_root) = project_root {
            let mut stmt = self.conn.prepare(
                r"
                SELECT r.relationship_id, r.source_task_id, r.target_task_id, r.kind, r.created_by, r.created_at, r.updated_at
                FROM task_relationships r
                JOIN tasks src ON src.task_id = r.source_task_id
                JOIN tasks tgt ON tgt.task_id = r.target_task_id
                WHERE src.project_root = ?1 AND tgt.project_root = ?1
                ORDER BY r.rowid
                ",
            )?;
            let rows = stmt.query_map([project_root], map_task_relationship)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(StoreError::from)
        } else {
            self.list_task_relationships(None)
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn apply_task_graph_action(
        &self,
        task_id: &str,
        action: OperatorActionKind,
        changed_by: &str,
        input: &super::TaskOperatorActionInput<'_>,
    ) -> StoreResult<Option<Task>> {
        let task = match action {
            OperatorActionKind::ResolveDependency => self.in_transaction(|conn| {
                let current_task = get_task_in_connection(conn, task_id)?;
                let related_task_id = input.related_task_id.ok_or_else(|| {
                    StoreError::Validation(
                        "resolve_dependency requires a related_task_id".to_string(),
                    )
                })?;
                let related_task = get_task_in_connection(conn, related_task_id)?;
                let relationship = find_task_relationship_between_in_connection(
                    conn,
                    task_id,
                    related_task_id,
                    TaskRelationshipKind::Blocks,
                )?
                .ok_or_else(|| {
                    StoreError::Validation(
                        "resolve_dependency requires an existing dependency relationship"
                            .to_string(),
                    )
                })?;
                delete_task_relationship_in_connection(conn, &relationship.relationship_id)?;
                let current_role = if relationship.source_task_id == task_id {
                    TaskRelationshipRole::Blocks
                } else {
                    TaskRelationshipRole::BlockedBy
                };
                let current_note = format!(
                    "relationship_id={}; action=resolve_dependency; kind={}; role={}; related_task_id={}; related_title={}",
                    relationship.relationship_id,
                    relationship.kind,
                    current_role,
                    related_task.task_id,
                    related_task.title
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::RelationshipUpdated,
                        actor: changed_by,
                        from_status: Some(current_task.status),
                        to_status: current_task.status,
                        verification_state: Some(current_task.verification_state),
                        owner_agent_id: current_task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(current_note.as_str()),
                    },
                )?;
                let inverse_role = if current_role == TaskRelationshipRole::Blocks {
                    TaskRelationshipRole::BlockedBy
                } else {
                    TaskRelationshipRole::Blocks
                };
                let related_note = format!(
                    "relationship_id={}; action=resolve_dependency; kind={}; role={}; related_task_id={}; related_title={}",
                    relationship.relationship_id,
                    relationship.kind,
                    inverse_role,
                    current_task.task_id,
                    current_task.title
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id: related_task_id,
                        event_type: TaskEventType::RelationshipUpdated,
                        actor: changed_by,
                        from_status: Some(related_task.status),
                        to_status: related_task.status,
                        verification_state: Some(related_task.verification_state),
                        owner_agent_id: related_task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(related_note.as_str()),
                    },
                )?;
                get_task_in_connection(conn, task_id)
            })?,
            OperatorActionKind::PromoteFollowUp => self.in_transaction(|conn| {
                let current_task = get_task_in_connection(conn, task_id)?;
                let related_task_id = input.related_task_id.ok_or_else(|| {
                    StoreError::Validation(
                        "promote_follow_up requires a related_task_id".to_string(),
                    )
                })?;
                let related_task = get_task_in_connection(conn, related_task_id)?;
                let relationship = find_task_relationship_between_in_connection(
                    conn,
                    task_id,
                    related_task_id,
                    TaskRelationshipKind::FollowUp,
                )?
                .ok_or_else(|| {
                    StoreError::Validation(
                        "promote_follow_up requires an existing follow-up relationship"
                            .to_string(),
                    )
                })?;
                delete_task_relationship_in_connection(conn, &relationship.relationship_id)?;
                let current_role = if relationship.source_task_id == task_id {
                    TaskRelationshipRole::FollowUpChild
                } else {
                    TaskRelationshipRole::FollowUpParent
                };
                let current_note = format!(
                    "relationship_id={}; action=promote_follow_up; kind={}; role={}; related_task_id={}; related_title={}",
                    relationship.relationship_id,
                    relationship.kind,
                    current_role,
                    related_task.task_id,
                    related_task.title
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id,
                        event_type: TaskEventType::RelationshipUpdated,
                        actor: changed_by,
                        from_status: Some(current_task.status),
                        to_status: current_task.status,
                        verification_state: Some(current_task.verification_state),
                        owner_agent_id: current_task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(current_note.as_str()),
                    },
                )?;
                let inverse_role = if current_role == TaskRelationshipRole::FollowUpChild {
                    TaskRelationshipRole::FollowUpParent
                } else {
                    TaskRelationshipRole::FollowUpChild
                };
                let related_note = format!(
                    "relationship_id={}; action=promote_follow_up; kind={}; role={}; related_task_id={}; related_title={}",
                    relationship.relationship_id,
                    relationship.kind,
                    inverse_role,
                    current_task.task_id,
                    current_task.title
                );
                record_task_event_in_connection(
                    conn,
                    &TaskEventWrite {
                        task_id: related_task_id,
                        event_type: TaskEventType::RelationshipUpdated,
                        actor: changed_by,
                        from_status: Some(related_task.status),
                        to_status: related_task.status,
                        verification_state: Some(related_task.verification_state),
                        owner_agent_id: related_task.owner_agent_id.as_deref(),
                        execution_action: None,
                        execution_duration_seconds: None,
                        note: Some(related_note.as_str()),
                    },
                )?;
                get_task_in_connection(conn, task_id)
            })?,
            OperatorActionKind::CloseFollowUpChain => self.in_transaction(|conn| {
                let current_task = get_task_in_connection(conn, task_id)?;
                let relationships = list_source_task_relationships_in_connection(
                    conn,
                    task_id,
                    TaskRelationshipKind::FollowUp,
                )?;
                if relationships.is_empty() {
                    return Err(StoreError::Validation(
                        "close_follow_up_chain requires follow-up child relationships".to_string(),
                    ));
                }

                let mut related_tasks = Vec::with_capacity(relationships.len());
                for relationship in &relationships {
                    let related_task = get_task_in_connection(conn, &relationship.target_task_id)?;
                    if is_open_task_status(related_task.status) {
                        return Err(StoreError::Validation(
                            "close_follow_up_chain requires all follow-up tasks to be terminal"
                                .to_string(),
                        ));
                    }
                    related_tasks.push((relationship.clone(), related_task));
                }

                for (relationship, related_task) in &related_tasks {
                    delete_task_relationship_in_connection(conn, &relationship.relationship_id)?;
                    let current_note = format!(
                        "relationship_id={}; action=close_follow_up_chain; kind={}; role={}; related_task_id={}; related_title={}",
                        relationship.relationship_id,
                        relationship.kind,
                        TaskRelationshipRole::FollowUpChild,
                        related_task.task_id,
                        related_task.title
                    );
                    record_task_event_in_connection(
                        conn,
                        &TaskEventWrite {
                            task_id,
                            event_type: TaskEventType::RelationshipUpdated,
                            actor: changed_by,
                            from_status: Some(current_task.status),
                            to_status: current_task.status,
                            verification_state: Some(current_task.verification_state),
                            owner_agent_id: current_task.owner_agent_id.as_deref(),
                            execution_action: None,
                            execution_duration_seconds: None,
                            note: Some(current_note.as_str()),
                        },
                    )?;
                    let related_note = format!(
                        "relationship_id={}; action=close_follow_up_chain; kind={}; role={}; related_task_id={}; related_title={}",
                        relationship.relationship_id,
                        relationship.kind,
                        TaskRelationshipRole::FollowUpParent,
                        current_task.task_id,
                        current_task.title
                    );
                    record_task_event_in_connection(
                        conn,
                        &TaskEventWrite {
                            task_id: &related_task.task_id,
                            event_type: TaskEventType::RelationshipUpdated,
                            actor: changed_by,
                            from_status: Some(related_task.status),
                            to_status: related_task.status,
                            verification_state: Some(related_task.verification_state),
                            owner_agent_id: related_task.owner_agent_id.as_deref(),
                            execution_action: None,
                            execution_duration_seconds: None,
                            note: Some(related_note.as_str()),
                        },
                    )?;
                }

                get_task_in_connection(conn, task_id)
            })?,
            _ => return Ok(None),
        };

        Ok(Some(task))
    }
}
