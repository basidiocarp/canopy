use super::*;

pub(crate) fn create_task_relationship_in_connection(
    conn: &Connection,
    source_task_id: &str,
    target_task_id: &str,
    kind: TaskRelationshipKind,
    created_by: &str,
) -> StoreResult<TaskRelationship> {
    let source_task = get_task_in_connection(conn, source_task_id)?;
    let target_task = get_task_in_connection(conn, target_task_id)?;
    if source_task_id == target_task_id {
        return Err(StoreError::Validation(
            "task relationships must link two different tasks".to_string(),
        ));
    }
    if source_task.project_root != target_task.project_root {
        return Err(StoreError::Validation(
            "task relationships must stay within the same project".to_string(),
        ));
    }
    if kind == TaskRelationshipKind::Parent {
        let existing_parent = conn
            .query_row(
                r"
                SELECT parent_task_id
                FROM tasks
                WHERE task_id = ?1
                LIMIT 1
                ",
                [source_task_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        if existing_parent.is_some() {
            return Err(StoreError::Validation(
                "task already has a parent".to_string(),
            ));
        }
        if parent_chain_contains_task_in_connection(conn, target_task_id, source_task_id)? {
            return Err(StoreError::Validation(
                "parent relationship would create a cycle".to_string(),
            ));
        }
    }
    let duplicate = conn
        .query_row(
            r"
            SELECT relationship_id
            FROM task_relationships
            WHERE source_task_id = ?1 AND target_task_id = ?2 AND kind = ?3
            ",
            params![source_task_id, target_task_id, kind.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if duplicate.is_some() {
        return Err(StoreError::Validation(
            "task relationship already exists".to_string(),
        ));
    }

    let relationship = TaskRelationship {
        relationship_id: Ulid::new().to_string(),
        source_task_id: source_task_id.to_string(),
        target_task_id: target_task_id.to_string(),
        kind,
        created_by: created_by.to_string(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    conn.execute(
        r"
        INSERT INTO task_relationships (
            relationship_id, source_task_id, target_task_id, kind, created_by, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ",
        params![
            relationship.relationship_id,
            relationship.source_task_id,
            relationship.target_task_id,
            relationship.kind.to_string(),
            relationship.created_by,
        ],
    )?;
    if kind == TaskRelationshipKind::Parent {
        conn.execute(
            r"
            UPDATE tasks
            SET parent_task_id = ?2,
                updated_at = CURRENT_TIMESTAMP
            WHERE task_id = ?1
            ",
            params![source_task_id, target_task_id],
        )?;
    }
    touch_task_in_connection(conn, source_task_id)?;
    touch_task_in_connection(conn, target_task_id)?;
    get_task_relationship_in_connection(conn, &relationship.relationship_id)
}

pub(crate) fn record_parent_relationship_in_connection(
    conn: &Connection,
    child_task_id: &str,
    parent_task_id: &str,
    created_by: &str,
) -> StoreResult<TaskRelationship> {
    let child_task = get_task_in_connection(conn, child_task_id)?;
    let parent_task = get_task_in_connection(conn, parent_task_id)?;
    let relationship = create_task_relationship_in_connection(
        conn,
        child_task_id,
        parent_task_id,
        TaskRelationshipKind::Parent,
        created_by,
    )?;
    let child_note = format!(
        "relationship_id={}; kind={}; role={}; related_task_id={}; related_title={}",
        relationship.relationship_id,
        relationship.kind,
        crate::models::TaskRelationshipRole::Parent,
        parent_task.task_id,
        parent_task.title
    );
    record_task_event_in_connection(
        conn,
        &TaskEventWrite {
            task_id: child_task_id,
            event_type: TaskEventType::RelationshipUpdated,
            actor: created_by,
            from_status: Some(child_task.status),
            to_status: child_task.status,
            verification_state: Some(child_task.verification_state),
            owner_agent_id: child_task.owner_agent_id.as_deref(),
            execution_action: None,
            execution_duration_seconds: None,
            note: Some(child_note.as_str()),
        },
    )?;
    let parent_note = format!(
        "relationship_id={}; kind={}; role={}; related_task_id={}; related_title={}",
        relationship.relationship_id,
        relationship.kind,
        crate::models::TaskRelationshipRole::Child,
        child_task.task_id,
        child_task.title
    );
    record_task_event_in_connection(
        conn,
        &TaskEventWrite {
            task_id: parent_task_id,
            event_type: TaskEventType::RelationshipUpdated,
            actor: created_by,
            from_status: Some(parent_task.status),
            to_status: parent_task.status,
            verification_state: Some(parent_task.verification_state),
            owner_agent_id: parent_task.owner_agent_id.as_deref(),
            execution_action: None,
            execution_duration_seconds: None,
            note: Some(parent_note.as_str()),
        },
    )?;
    Ok(relationship)
}

pub(crate) fn find_task_relationship_between_in_connection(
    conn: &Connection,
    task_id: &str,
    related_task_id: &str,
    kind: TaskRelationshipKind,
) -> StoreResult<Option<TaskRelationship>> {
    conn.query_row(
        r"
        SELECT relationship_id, source_task_id, target_task_id, kind, created_by, created_at, updated_at
        FROM task_relationships
        WHERE kind = ?1
          AND (
            (source_task_id = ?2 AND target_task_id = ?3)
            OR (source_task_id = ?3 AND target_task_id = ?2)
          )
        ORDER BY created_at DESC
        LIMIT 1
        ",
        params![kind.to_string(), task_id, related_task_id],
        map_task_relationship,
    )
    .optional()
    .map_err(StoreError::from)
}

pub(crate) fn list_source_task_relationships_in_connection(
    conn: &Connection,
    task_id: &str,
    kind: TaskRelationshipKind,
) -> StoreResult<Vec<TaskRelationship>> {
    let mut statement = conn.prepare(
        r"
        SELECT relationship_id, source_task_id, target_task_id, kind, created_by, created_at, updated_at
        FROM task_relationships
        WHERE source_task_id = ?1 AND kind = ?2
        ORDER BY created_at ASC
        ",
    )?;
    let rows = statement.query_map(params![task_id, kind.to_string()], map_task_relationship)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

pub(crate) fn parent_chain_contains_task_in_connection(
    conn: &Connection,
    start_task_id: &str,
    candidate_ancestor_task_id: &str,
) -> StoreResult<bool> {
    let mut stmt = conn.prepare(
        r"
        WITH RECURSIVE ancestry(task_id) AS (
            SELECT target_task_id
            FROM task_relationships
            WHERE source_task_id = ?1
              AND kind = 'parent'
            UNION
            SELECT rel.target_task_id
            FROM task_relationships rel
            INNER JOIN ancestry ON rel.source_task_id = ancestry.task_id
            WHERE rel.kind = 'parent'
        )
        SELECT 1
        FROM ancestry
        WHERE task_id = ?2
        LIMIT 1
        ",
    )?;
    stmt.exists(params![start_task_id, candidate_ancestor_task_id])
        .map_err(StoreError::from)
}

pub(crate) fn delete_task_relationship_in_connection(
    conn: &Connection,
    relationship_id: &str,
) -> StoreResult<()> {
    let relationship = get_task_relationship_in_connection(conn, relationship_id)?;
    let deleted = conn.execute(
        "DELETE FROM task_relationships WHERE relationship_id = ?1",
        [relationship_id],
    )?;
    if deleted == 0 {
        return Err(StoreError::NotFound("task relationship"));
    }
    if let Some(child_task_id) = match relationship.kind {
        TaskRelationshipKind::Parent => Some(relationship.source_task_id.as_str()),
        TaskRelationshipKind::FollowUp | TaskRelationshipKind::Blocks => None,
    } {
        conn.execute(
            r"
            UPDATE tasks
            SET parent_task_id = NULL,
                updated_at = CURRENT_TIMESTAMP
            WHERE task_id = ?1
            ",
            [child_task_id],
        )?;
    }
    touch_task_in_connection(conn, &relationship.source_task_id)?;
    touch_task_in_connection(conn, &relationship.target_task_id)?;
    Ok(())
}
