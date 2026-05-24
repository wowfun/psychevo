#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEdgeStatus {
    Open,
    Closed,
}

impl AgentEdgeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "closed" => Self::Closed,
            _ => Self::Open,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentEdgeRecord {
    pub parent_session_id: String,
    pub child_session_id: String,
    pub status: AgentEdgeStatus,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub metadata: Option<Value>,
}

impl SqliteStore {
    pub fn upsert_agent_edge(
        &self,
        parent_session_id: &str,
        child_session_id: &str,
        status: AgentEdgeStatus,
        metadata: Option<Value>,
    ) -> Result<()> {
        let now = now_ms();
        let metadata_json = optional_json_string(&metadata)?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO agent_edges (
                    parent_session_id, child_session_id, status,
                    created_at_ms, updated_at_ms, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?4, ?5)
                ON CONFLICT(child_session_id) DO UPDATE SET
                    parent_session_id = excluded.parent_session_id,
                    status = excluded.status,
                    updated_at_ms = excluded.updated_at_ms,
                    metadata_json = excluded.metadata_json
                "#,
                params![
                    parent_session_id,
                    child_session_id,
                    status.as_str(),
                    now,
                    metadata_json
                ],
            )?;
            Ok(())
        })
    }

    pub fn set_agent_edge_status(
        &self,
        child_session_id: &str,
        status: AgentEdgeStatus,
    ) -> Result<()> {
        let now = now_ms();
        self.write_retry(|conn| {
            conn.execute(
                "UPDATE agent_edges SET status = ?1, updated_at_ms = ?2 WHERE child_session_id = ?3",
                params![status.as_str(), now, child_session_id],
            )?;
            Ok(())
        })
    }

    pub fn list_agent_edges(&self) -> Result<Vec<AgentEdgeRecord>> {
        self.query_agent_edges(None)
    }

    pub fn list_agent_edges_for_parent(
        &self,
        parent_session_id: &str,
    ) -> Result<Vec<AgentEdgeRecord>> {
        self.query_agent_edges(Some(parent_session_id))
    }

    pub fn find_agent_edge(&self, target: &str) -> Result<Option<AgentEdgeRecord>> {
        let target = target.trim();
        if target.is_empty() {
            return Ok(None);
        }
        for edge in self.list_agent_edges()? {
            if edge.child_session_id == target || agent_edge_metadata_matches(&edge, target) {
                return Ok(Some(edge));
            }
        }
        Ok(None)
    }

    pub fn close_agent_edge_subtree(&self, child_session_id: &str) -> Result<()> {
        let mut queue = vec![child_session_id.to_string()];
        let mut closed = BTreeSet::new();
        while let Some(current) = queue.pop() {
            if !closed.insert(current.clone()) {
                continue;
            }
            self.set_agent_edge_status(&current, AgentEdgeStatus::Closed)?;
            for child in self.list_agent_edges_for_parent(&current)? {
                queue.push(child.child_session_id);
            }
        }
        Ok(())
    }

    pub(crate) fn query_agent_edges(
        &self,
        parent_session_id: Option<&str>,
    ) -> Result<Vec<AgentEdgeRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let sql = match parent_session_id {
            Some(_) => {
                r#"
                SELECT parent_session_id, child_session_id, status,
                       created_at_ms, updated_at_ms, metadata_json
                FROM agent_edges
                WHERE parent_session_id = ?1
                ORDER BY updated_at_ms DESC, created_at_ms DESC
                "#
            }
            None => {
                r#"
                SELECT parent_session_id, child_session_id, status,
                       created_at_ms, updated_at_ms, metadata_json
                FROM agent_edges
                ORDER BY updated_at_ms DESC, created_at_ms DESC
                "#
            }
        };
        let mut stmt = conn.prepare(sql)?;
        let mut records = Vec::new();
        match parent_session_id {
            Some(parent) => {
                let rows = stmt.query_map(params![parent], agent_edge_from_row)?;
                for row in rows {
                    records.push(row?);
                }
            }
            None => {
                let rows = stmt.query_map([], agent_edge_from_row)?;
                for row in rows {
                    records.push(row?);
                }
            }
        }
        Ok(records)
    }
}

pub(crate) fn agent_edge_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentEdgeRecord> {
    let status: String = row.get(2)?;
    let metadata_json: Option<String> = row.get(5)?;
    Ok(AgentEdgeRecord {
        parent_session_id: row.get(0)?,
        child_session_id: row.get(1)?,
        status: AgentEdgeStatus::parse(&status),
        created_at_ms: row.get(3)?,
        updated_at_ms: row.get(4)?,
        metadata: metadata_json
            .map(|value| serde_json::from_str(&value).map_err(json_to_sql))
            .transpose()?,
    })
}

pub(crate) fn agent_edge_metadata_matches(edge: &AgentEdgeRecord, target: &str) -> bool {
    let Some(metadata) = edge.metadata.as_ref().and_then(Value::as_object) else {
        return false;
    };
    let Some(agent) = metadata.get("agent").and_then(Value::as_object) else {
        return false;
    };
    agent
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|value| value == target)
        || agent
            .get("task_name")
            .and_then(Value::as_str)
            .is_some_and(|value| value == target)
}
