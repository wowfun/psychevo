use std::collections::BTreeSet;

use psychevo_agent_core::now_ms;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

use crate::error::Result;

use super::StateRuntime;
use super::store_message_fields::optional_json_string;

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
    pub async fn upsert_agent_edge(
        &self,
        parent_session_id: &str,
        child_session_id: &str,
        status: AgentEdgeStatus,
        metadata: Option<Value>,
    ) -> Result<()> {
        let now = now_ms();
        let metadata_json = optional_json_string(&metadata)?;
        self.agent_write(
            sqlx::query(
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
            )
            .bind(parent_session_id)
            .bind(child_session_id)
            .bind(status.as_str())
            .bind(now)
            .bind(metadata_json),
        )
        .await
    }

    pub async fn set_agent_edge_status(
        &self,
        child_session_id: &str,
        status: AgentEdgeStatus,
    ) -> Result<()> {
        let now = now_ms();
        self.agent_write(
            sqlx::query(
                "UPDATE agent_edges SET status = ?1, updated_at_ms = ?2 WHERE child_session_id = ?3",
            )
            .bind(status.as_str())
            .bind(now)
            .bind(child_session_id),
        )
        .await
    }

    pub async fn list_agent_edges(&self) -> Result<Vec<AgentEdgeRecord>> {
        self.query_agent_edges(None).await
    }

    pub async fn list_agent_edges_for_parent(
        &self,
        parent_session_id: &str,
    ) -> Result<Vec<AgentEdgeRecord>> {
        self.query_agent_edges(Some(parent_session_id)).await
    }

    pub async fn find_agent_edge(&self, target: &str) -> Result<Option<AgentEdgeRecord>> {
        let target = target.trim();
        if target.is_empty() {
            return Ok(None);
        }
        for edge in self.list_agent_edges().await? {
            if edge.child_session_id == target || agent_edge_metadata_matches(&edge, target) {
                return Ok(Some(edge));
            }
        }
        Ok(None)
    }

    pub async fn close_agent_edge_subtree(&self, child_session_id: &str) -> Result<()> {
        let mut queue = vec![child_session_id.to_string()];
        let mut closed = BTreeSet::new();
        while let Some(current) = queue.pop() {
            if !closed.insert(current.clone()) {
                continue;
            }
            self.set_agent_edge_status(&current, AgentEdgeStatus::Closed)
                .await?;
            for child in self.list_agent_edges_for_parent(&current).await? {
                queue.push(child.child_session_id);
            }
        }
        Ok(())
    }

    pub async fn create_agent_team_run(
        &self,
        input: AgentTeamRunInput<'_>,
    ) -> Result<AgentTeamRunRecord> {
        let now = now_ms();
        let members_json = serde_json::to_string(&input.members)?;
        let metadata_json = optional_json_string(&input.metadata)?;
        let max_parallel_agents = i64::try_from(input.max_parallel_agents).unwrap_or(i64::MAX);
        self.agent_write(
            sqlx::query(
                r#"
                INSERT INTO agent_team_runs (
                    id, parent_session_id, mission_run_id, team_name, description,
                    source_path, leader_agent_name, members_json, max_parallel_agents,
                    status, started_at_ms, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                "#,
            )
            .bind(input.id)
            .bind(input.parent_session_id)
            .bind(input.mission_run_id)
            .bind(input.team_name)
            .bind(input.description)
            .bind(input.source_path)
            .bind(input.leader_agent_name)
            .bind(members_json)
            .bind(max_parallel_agents)
            .bind(input.status)
            .bind(now)
            .bind(metadata_json),
        )
        .await?;
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

    pub async fn create_agent_mission_run(
        &self,
        input: AgentMissionRunInput<'_>,
    ) -> Result<AgentMissionRunRecord> {
        let now = now_ms();
        let metadata_json = optional_json_string(&input.metadata)?;
        self.agent_write(
            sqlx::query(
                r#"
                INSERT INTO agent_mission_runs (
                    id, parent_session_id, team_run_id, team_name, goal,
                    lead_agent_name, status, started_at_ms, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                "#,
            )
            .bind(input.id)
            .bind(input.parent_session_id)
            .bind(input.team_run_id)
            .bind(input.team_name)
            .bind(input.goal)
            .bind(input.lead_agent_name)
            .bind(input.status)
            .bind(now)
            .bind(metadata_json),
        )
        .await?;
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

    pub async fn update_agent_team_run_status(
        &self,
        id: &str,
        status: &str,
        final_summary: Option<&str>,
        ended: bool,
    ) -> Result<()> {
        let ended_at_ms = ended.then(now_ms);
        self.agent_write(
            sqlx::query(
                r#"
                UPDATE agent_team_runs
                SET status = ?1,
                    final_summary = COALESCE(?2, final_summary),
                    ended_at_ms = COALESCE(?3, ended_at_ms)
                WHERE id = ?4
                "#,
            )
            .bind(status)
            .bind(final_summary)
            .bind(ended_at_ms)
            .bind(id),
        )
        .await
    }

    pub async fn update_agent_mission_run_status(
        &self,
        id: &str,
        status: &str,
        final_summary: Option<&str>,
        ended: bool,
    ) -> Result<()> {
        let ended_at_ms = ended.then(now_ms);
        self.agent_write(
            sqlx::query(
                r#"
                UPDATE agent_mission_runs
                SET status = ?1,
                    final_summary = COALESCE(?2, final_summary),
                    ended_at_ms = COALESCE(?3, ended_at_ms)
                WHERE id = ?4
                "#,
            )
            .bind(status)
            .bind(final_summary)
            .bind(ended_at_ms)
            .bind(id),
        )
        .await
    }

    pub async fn list_agent_team_runs_for_parent(
        &self,
        parent_session_id: &str,
    ) -> Result<Vec<AgentTeamRunRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT id, parent_session_id, mission_run_id, team_name, description,
                   source_path, leader_agent_name, members_json, max_parallel_agents,
                   status, started_at_ms, ended_at_ms, final_summary, metadata_json
            FROM agent_team_runs
            WHERE parent_session_id = ?1
            ORDER BY started_at_ms DESC
            "#,
            )
            .bind(parent_session_id)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| agent_team_run_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn list_agent_mission_runs_for_parent(
        &self,
        parent_session_id: &str,
    ) -> Result<Vec<AgentMissionRunRecord>> {
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
                r#"
            SELECT id, parent_session_id, team_run_id, team_name, goal,
                   lead_agent_name, status, started_at_ms, ended_at_ms,
                   final_summary, metadata_json
            FROM agent_mission_runs
            WHERE parent_session_id = ?1
            ORDER BY started_at_ms DESC
            "#,
            )
            .bind(parent_session_id)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| agent_mission_run_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn find_active_agent_team_run(
        &self,
        parent_session_id: &str,
    ) -> Result<Option<AgentTeamRunRecord>> {
        Ok(self
            .list_agent_team_runs_for_parent(parent_session_id)
            .await?
            .into_iter()
            .find(|record| record.ended_at_ms.is_none()))
    }

    pub async fn find_active_agent_mission_run(
        &self,
        parent_session_id: &str,
    ) -> Result<Option<AgentMissionRunRecord>> {
        Ok(self
            .list_agent_mission_runs_for_parent(parent_session_id)
            .await?
            .into_iter()
            .find(|record| record.ended_at_ms.is_none()))
    }

    pub(crate) async fn query_agent_edges(
        &self,
        parent_session_id: Option<&str>,
    ) -> Result<Vec<AgentEdgeRecord>> {
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
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let mut query = sqlx::query(sql);
            if let Some(parent) = parent_session_id {
                query = query.bind(parent);
            }
            let rows = query.fetch_all(&mut *conn).await?;
            rows.into_iter()
                .map(|row| agent_edge_from_row(&row))
                .collect()
        })
        .await
    }

    async fn agent_write<'q>(
        &self,
        query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    ) -> Result<()> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            query.execute(&mut *tx).await?;
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(())
        })
        .await
    }
}

pub(crate) fn agent_team_run_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<AgentTeamRunRecord> {
    let members_json: String = row.try_get(7)?;
    let metadata_json: Option<String> = row.try_get(13)?;
    let max_parallel_agents: i64 = row.try_get(8)?;
    Ok(AgentTeamRunRecord {
        id: row.try_get(0)?,
        parent_session_id: row.try_get(1)?,
        mission_run_id: row.try_get(2)?,
        team_name: row.try_get(3)?,
        description: row.try_get(4)?,
        source_path: row.try_get(5)?,
        leader_agent_name: row.try_get(6)?,
        members: serde_json::from_str(&members_json)?,
        max_parallel_agents: max_parallel_agents.max(0) as u64,
        status: row.try_get(9)?,
        started_at_ms: row.try_get(10)?,
        ended_at_ms: row.try_get(11)?,
        final_summary: row.try_get(12)?,
        metadata: metadata_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
    })
}

pub(crate) fn agent_mission_run_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<AgentMissionRunRecord> {
    let metadata_json: Option<String> = row.try_get(10)?;
    Ok(AgentMissionRunRecord {
        id: row.try_get(0)?,
        parent_session_id: row.try_get(1)?,
        team_run_id: row.try_get(2)?,
        team_name: row.try_get(3)?,
        goal: row.try_get(4)?,
        lead_agent_name: row.try_get(5)?,
        status: row.try_get(6)?,
        started_at_ms: row.try_get(7)?,
        ended_at_ms: row.try_get(8)?,
        final_summary: row.try_get(9)?,
        metadata: metadata_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
    })
}

pub(crate) fn agent_edge_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<AgentEdgeRecord> {
    let status: String = row.try_get(2)?;
    let metadata_json: Option<String> = row.try_get(5)?;
    Ok(AgentEdgeRecord {
        parent_session_id: row.try_get(0)?,
        child_session_id: row.try_get(1)?,
        status: AgentEdgeStatus::parse(&status),
        created_at_ms: row.try_get(3)?,
        updated_at_ms: row.try_get(4)?,
        metadata: metadata_json
            .map(|value| serde_json::from_str(&value))
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
