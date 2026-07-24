use std::collections::BTreeSet;

use psychevo_agent_core::now_ms;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;

use super::StateRuntime;
use super::store_message_fields::optional_json_string;
use super::store_metadata::json_to_sql;

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

#[derive(Debug, Clone, PartialEq)]
pub struct AgentTeamRunInput<'a> {
    pub id: &'a str,
    pub parent_session_id: &'a str,
    pub mission_run_id: Option<&'a str>,
    pub team_name: &'a str,
    pub description: Option<&'a str>,
    pub source_path: Option<&'a str>,
    pub leader_agent_name: &'a str,
    pub members: Value,
    pub max_parallel_agents: u64,
    pub status: &'a str,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentTeamRunRecord {
    pub id: String,
    pub parent_session_id: String,
    pub mission_run_id: Option<String>,
    pub team_name: String,
    pub description: Option<String>,
    pub source_path: Option<String>,
    pub leader_agent_name: String,
    pub members: Value,
    pub max_parallel_agents: u64,
    pub status: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub final_summary: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentMissionRunInput<'a> {
    pub id: &'a str,
    pub parent_session_id: &'a str,
    pub team_run_id: Option<&'a str>,
    pub team_name: Option<&'a str>,
    pub goal: &'a str,
    pub lead_agent_name: &'a str,
    pub status: &'a str,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentMissionRunRecord {
    pub id: String,
    pub parent_session_id: String,
    pub team_run_id: Option<String>,
    pub team_name: Option<String>,
    pub goal: String,
    pub lead_agent_name: String,
    pub status: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub final_summary: Option<String>,
    pub metadata: Option<Value>,
}

impl StateRuntime {
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

    pub fn create_agent_team_run(
        &self,
        input: AgentTeamRunInput<'_>,
    ) -> Result<AgentTeamRunRecord> {
        let now = now_ms();
        let members_json = serde_json::to_string(&input.members)?;
        let metadata_json = optional_json_string(&input.metadata)?;
        let max_parallel_agents = i64::try_from(input.max_parallel_agents).unwrap_or(i64::MAX);
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO agent_team_runs (
                    id, parent_session_id, mission_run_id, team_name, description,
                    source_path, leader_agent_name, members_json, max_parallel_agents,
                    status, started_at_ms, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                "#,
                params![
                    input.id,
                    input.parent_session_id,
                    input.mission_run_id,
                    input.team_name,
                    input.description,
                    input.source_path,
                    input.leader_agent_name,
                    members_json,
                    max_parallel_agents,
                    input.status,
                    now,
                    metadata_json,
                ],
            )?;
            Ok(())
        })?;
        Ok(AgentTeamRunRecord {
            id: input.id.to_string(),
            parent_session_id: input.parent_session_id.to_string(),
            mission_run_id: input.mission_run_id.map(str::to_string),
            team_name: input.team_name.to_string(),
            description: input.description.map(str::to_string),
            source_path: input.source_path.map(str::to_string),
            leader_agent_name: input.leader_agent_name.to_string(),
            members: input.members,
            max_parallel_agents: input.max_parallel_agents,
            status: input.status.to_string(),
            started_at_ms: now,
            ended_at_ms: None,
            final_summary: None,
            metadata: input.metadata,
        })
    }

    pub fn create_agent_mission_run(
        &self,
        input: AgentMissionRunInput<'_>,
    ) -> Result<AgentMissionRunRecord> {
        let now = now_ms();
        let metadata_json = optional_json_string(&input.metadata)?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO agent_mission_runs (
                    id, parent_session_id, team_run_id, team_name, goal,
                    lead_agent_name, status, started_at_ms, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                "#,
                params![
                    input.id,
                    input.parent_session_id,
                    input.team_run_id,
                    input.team_name,
                    input.goal,
                    input.lead_agent_name,
                    input.status,
                    now,
                    metadata_json,
                ],
            )?;
            Ok(())
        })?;
        Ok(AgentMissionRunRecord {
            id: input.id.to_string(),
            parent_session_id: input.parent_session_id.to_string(),
            team_run_id: input.team_run_id.map(str::to_string),
            team_name: input.team_name.map(str::to_string),
            goal: input.goal.to_string(),
            lead_agent_name: input.lead_agent_name.to_string(),
            status: input.status.to_string(),
            started_at_ms: now,
            ended_at_ms: None,
            final_summary: None,
            metadata: input.metadata,
        })
    }

    pub fn update_agent_team_run_status(
        &self,
        id: &str,
        status: &str,
        final_summary: Option<&str>,
        ended: bool,
    ) -> Result<()> {
        let ended_at_ms = ended.then(now_ms);
        self.write_retry(|conn| {
            conn.execute(
                r#"
                UPDATE agent_team_runs
                SET status = ?1,
                    final_summary = COALESCE(?2, final_summary),
                    ended_at_ms = COALESCE(?3, ended_at_ms)
                WHERE id = ?4
                "#,
                params![status, final_summary, ended_at_ms, id],
            )?;
            Ok(())
        })
    }

    pub fn update_agent_mission_run_status(
        &self,
        id: &str,
        status: &str,
        final_summary: Option<&str>,
        ended: bool,
    ) -> Result<()> {
        let ended_at_ms = ended.then(now_ms);
        self.write_retry(|conn| {
            conn.execute(
                r#"
                UPDATE agent_mission_runs
                SET status = ?1,
                    final_summary = COALESCE(?2, final_summary),
                    ended_at_ms = COALESCE(?3, ended_at_ms)
                WHERE id = ?4
                "#,
                params![status, final_summary, ended_at_ms, id],
            )?;
            Ok(())
        })
    }

    pub fn list_agent_team_runs_for_parent(
        &self,
        parent_session_id: &str,
    ) -> Result<Vec<AgentTeamRunRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, parent_session_id, mission_run_id, team_name, description,
                   source_path, leader_agent_name, members_json, max_parallel_agents,
                   status, started_at_ms, ended_at_ms, final_summary, metadata_json
            FROM agent_team_runs
            WHERE parent_session_id = ?1
            ORDER BY started_at_ms DESC
            "#,
        )?;
        let rows = stmt.query_map(params![parent_session_id], agent_team_run_from_row)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    pub fn list_agent_mission_runs_for_parent(
        &self,
        parent_session_id: &str,
    ) -> Result<Vec<AgentMissionRunRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, parent_session_id, team_run_id, team_name, goal,
                   lead_agent_name, status, started_at_ms, ended_at_ms,
                   final_summary, metadata_json
            FROM agent_mission_runs
            WHERE parent_session_id = ?1
            ORDER BY started_at_ms DESC
            "#,
        )?;
        let rows = stmt.query_map(params![parent_session_id], agent_mission_run_from_row)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    pub fn find_active_agent_team_run(
        &self,
        parent_session_id: &str,
    ) -> Result<Option<AgentTeamRunRecord>> {
        Ok(self
            .list_agent_team_runs_for_parent(parent_session_id)?
            .into_iter()
            .find(|record| record.ended_at_ms.is_none()))
    }

    pub fn find_active_agent_mission_run(
        &self,
        parent_session_id: &str,
    ) -> Result<Option<AgentMissionRunRecord>> {
        Ok(self
            .list_agent_mission_runs_for_parent(parent_session_id)?
            .into_iter()
            .find(|record| record.ended_at_ms.is_none()))
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

pub(crate) fn agent_team_run_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentTeamRunRecord> {
    let members_json: String = row.get(7)?;
    let metadata_json: Option<String> = row.get(13)?;
    let max_parallel_agents: i64 = row.get(8)?;
    Ok(AgentTeamRunRecord {
        id: row.get(0)?,
        parent_session_id: row.get(1)?,
        mission_run_id: row.get(2)?,
        team_name: row.get(3)?,
        description: row.get(4)?,
        source_path: row.get(5)?,
        leader_agent_name: row.get(6)?,
        members: serde_json::from_str(&members_json).map_err(json_to_sql)?,
        max_parallel_agents: max_parallel_agents.max(0) as u64,
        status: row.get(9)?,
        started_at_ms: row.get(10)?,
        ended_at_ms: row.get(11)?,
        final_summary: row.get(12)?,
        metadata: metadata_json
            .map(|value| serde_json::from_str(&value).map_err(json_to_sql))
            .transpose()?,
    })
}

pub(crate) fn agent_mission_run_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentMissionRunRecord> {
    let metadata_json: Option<String> = row.get(10)?;
    Ok(AgentMissionRunRecord {
        id: row.get(0)?,
        parent_session_id: row.get(1)?,
        team_run_id: row.get(2)?,
        team_name: row.get(3)?,
        goal: row.get(4)?,
        lead_agent_name: row.get(5)?,
        status: row.get(6)?,
        started_at_ms: row.get(7)?,
        ended_at_ms: row.get(8)?,
        final_summary: row.get(9)?,
        metadata: metadata_json
            .map(|value| serde_json::from_str(&value).map_err(json_to_sql))
            .transpose()?,
    })
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
