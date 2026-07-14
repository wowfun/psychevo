use psychevo_agent_core::now_ms;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{Error, Result};

use super::{
    AutomationRunFinishInput, AutomationRunRecord, AutomationRunRecoveryCandidate,
    AutomationTaskInput, AutomationTaskRecord, SqliteStore,
};

impl SqliteStore {
    pub fn upsert_automation_task(
        &self,
        input: AutomationTaskInput,
    ) -> Result<AutomationTaskRecord> {
        let id = input.id.unwrap_or_else(|| Uuid::now_v7().to_string());
        let now = now_ms();
        let schedule_json = serde_json::to_string(&input.schedule)?;
        let execution_json = serde_json::to_string(&input.execution)?;
        self.write_retry(|conn| {
            conn.execute(
                r#"
                INSERT INTO automations (
                    id, cwd, kind, target_thread_id, title, prompt, schedule_json,
                    enabled, execution_json, model, reasoning_effort, source_key,
                    created_at_ms, updated_at_ms, next_run_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, ?14)
                ON CONFLICT(id) DO UPDATE SET
                    cwd = excluded.cwd,
                    kind = excluded.kind,
                    target_thread_id = excluded.target_thread_id,
                    title = excluded.title,
                    prompt = excluded.prompt,
                    schedule_json = excluded.schedule_json,
                    enabled = excluded.enabled,
                    execution_json = excluded.execution_json,
                    model = excluded.model,
                    reasoning_effort = excluded.reasoning_effort,
                    source_key = excluded.source_key,
                    updated_at_ms = excluded.updated_at_ms,
                    next_run_at_ms = excluded.next_run_at_ms
                "#,
                params![
                    id,
                    input.cwd,
                    input.kind,
                    input.target_thread_id,
                    input.title,
                    input.prompt,
                    schedule_json,
                    if input.enabled { 1_i64 } else { 0_i64 },
                    execution_json,
                    input.model,
                    input.reasoning_effort,
                    input.source_key,
                    now,
                    input.next_run_at_ms,
                ],
            )?;
            Ok(())
        })?;
        self.automation_task(&id)?
            .ok_or_else(|| Error::Message(format!("automation task not found after upsert: {id}")))
    }

    pub fn automation_task(&self, id: &str) -> Result<Option<AutomationTaskRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            automation_task_select_sql("WHERE id = ?1").as_str(),
            params![id],
            automation_task_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn automation_tasks_for_cwd(&self, cwd: &str) -> Result<Vec<AutomationTaskRecord>> {
        self.automation_tasks_for_optional_cwd(Some(cwd))
    }

    pub fn automation_tasks_for_optional_cwd(
        &self,
        cwd: Option<&str>,
    ) -> Result<Vec<AutomationTaskRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            automation_task_select_sql(
                "WHERE (?1 IS NULL OR cwd = ?1)
                 ORDER BY enabled DESC, next_run_at_ms IS NULL, next_run_at_ms ASC, updated_at_ms DESC",
            )
            .as_str(),
        )?;
        let rows = stmt.query_map(params![cwd], automation_task_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn due_automation_tasks(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<AutomationTaskRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            automation_task_select_sql(
                "WHERE enabled = 1 AND next_run_at_ms IS NOT NULL AND next_run_at_ms <= ?1
                 ORDER BY next_run_at_ms ASC, updated_at_ms ASC
                 LIMIT ?2",
            )
            .as_str(),
        )?;
        let rows = stmt.query_map(params![now_ms, limit as i64], automation_task_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn delete_automation_task(&self, id: &str) -> Result<bool> {
        let changed = self.write_retry(|conn| {
            conn.execute("DELETE FROM automations WHERE id = ?1", params![id])
        })?;
        Ok(changed > 0)
    }

    pub fn claim_automation_run(
        &self,
        automation_id: &str,
        trigger: &str,
    ) -> Result<Option<AutomationRunRecord>> {
        let id = Uuid::now_v7().to_string();
        let now = now_ms();
        let inserted = self.write_retry(|conn| {
            let running: Option<String> = conn
                .query_row(
                    "SELECT id FROM automation_runs WHERE automation_id = ?1 AND status = 'running' LIMIT 1",
                    params![automation_id],
                    |row| row.get(0),
                )
                .optional()?;
            if running.is_some() {
                return Ok(false);
            }
            conn.execute(
                r#"
                INSERT INTO automation_runs (
                    id, automation_id, trigger, status, started_at_ms
                ) VALUES (?1, ?2, ?3, 'running', ?4)
                "#,
                params![id, automation_id, trigger, now],
            )?;
            conn.execute(
                r#"
                UPDATE automations
                SET last_run_at_ms = ?2,
                    last_status = 'running',
                    last_error = NULL,
                    updated_at_ms = ?2
                WHERE id = ?1
                "#,
                params![automation_id, now],
            )?;
            Ok(true)
        })?;
        if !inserted {
            return Ok(None);
        }
        self.automation_run(&id)
    }

    pub fn automation_run(&self, id: &str) -> Result<Option<AutomationRunRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        conn.query_row(
            automation_run_select_sql("WHERE id = ?1").as_str(),
            params![id],
            automation_run_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn automation_runs_for_task(
        &self,
        automation_id: &str,
        limit: usize,
    ) -> Result<Vec<AutomationRunRecord>> {
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            automation_run_select_sql(
                "WHERE automation_id = ?1 ORDER BY started_at_ms DESC LIMIT ?2",
            )
            .as_str(),
        )?;
        let rows = stmt.query_map(
            params![automation_id, limit as i64],
            automation_run_from_row,
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn stale_automation_runs_for_recovery(
        &self,
        now_ms: i64,
        stale_after_ms: i64,
        limit: usize,
    ) -> Result<Vec<AutomationRunRecoveryCandidate>> {
        let stale_before_ms = now_ms.saturating_sub(stale_after_ms);
        let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT
                a.id, a.cwd, a.kind, a.target_thread_id, a.title, a.prompt,
                a.schedule_json, a.enabled, a.execution_json, a.model,
                a.reasoning_effort, a.source_key, a.created_at_ms, a.updated_at_ms,
                a.last_run_at_ms, a.next_run_at_ms, a.last_status, a.last_error,
                r.id, r.automation_id, r.trigger, r.status, r.started_at_ms,
                r.completed_at_ms, r.thread_id, r.source_key, r.error, r.metadata_json
            FROM automation_runs r
            INNER JOIN automations a ON a.id = r.automation_id
            WHERE r.status = 'running'
              AND r.started_at_ms <= ?1
              AND NOT EXISTS (
                SELECT 1
                FROM gateway_activities g
                WHERE g.status IN ('running', 'queued')
                  AND g.lease_expires_at_ms >= ?2
                  AND (
                    (r.thread_id IS NOT NULL AND g.thread_id = r.thread_id)
                    OR (r.source_key IS NOT NULL AND g.source_key = r.source_key)
                    OR (a.target_thread_id IS NOT NULL AND g.thread_id = a.target_thread_id)
                    OR (a.source_key IS NOT NULL AND g.source_key = a.source_key)
                  )
              )
            ORDER BY r.started_at_ms ASC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(params![stale_before_ms, now_ms, limit as i64], |row| {
            Ok(AutomationRunRecoveryCandidate {
                task: automation_task_from_row(row)?,
                run: automation_run_from_row_at(row, 18)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn finish_automation_run(
        &self,
        input: AutomationRunFinishInput<'_>,
    ) -> Result<Option<AutomationRunRecord>> {
        let now = now_ms();
        let metadata_json = input
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let error = input.error.map(bounded_automation_error);
        let changed = self.write_retry(|conn| {
            let automation_id: Option<String> = conn
                .query_row(
                    "SELECT automation_id FROM automation_runs WHERE id = ?1",
                    params![input.run_id],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(automation_id) = automation_id else {
                return Ok(false);
            };
            let changed = conn.execute(
                r#"
                UPDATE automation_runs
                SET status = ?2,
                    completed_at_ms = ?3,
                    thread_id = ?4,
                    source_key = ?5,
                    error = ?6,
                    metadata_json = ?7
                WHERE id = ?1 AND status = 'running'
                "#,
                params![
                    input.run_id,
                    input.status,
                    now,
                    input.thread_id,
                    input.source_key,
                    error,
                    metadata_json,
                ],
            )?;
            if changed == 0 {
                return Ok(false);
            }
            conn.execute(
                r#"
                UPDATE automations
                SET last_status = ?2,
                    last_error = ?3,
                    next_run_at_ms = ?4,
                    source_key = COALESCE(?5, source_key),
                    updated_at_ms = ?6
                WHERE id = ?1
                "#,
                params![
                    automation_id,
                    input.status,
                    error,
                    input.next_run_at_ms,
                    input.source_key,
                    now
                ],
            )?;
            Ok(true)
        })?;
        if !changed {
            return self.automation_run(input.run_id);
        }
        self.automation_run(input.run_id)
    }
}

fn automation_task_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        SELECT id, cwd, kind, target_thread_id, title, prompt, schedule_json,
               enabled, execution_json, model, reasoning_effort, source_key,
               created_at_ms, updated_at_ms, last_run_at_ms, next_run_at_ms,
               last_status, last_error
        FROM automations
        {where_clause}
        "#
    )
}

fn automation_run_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        SELECT id, automation_id, trigger, status, started_at_ms, completed_at_ms,
               thread_id, source_key, error, metadata_json
        FROM automation_runs
        {where_clause}
        "#
    )
}

fn automation_task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationTaskRecord> {
    let schedule_json: String = row.get(6)?;
    let execution_json: String = row.get(8)?;
    let schedule = json_from_column(&schedule_json, 6)?;
    let execution = json_from_column(&execution_json, 8)?;
    let enabled: i64 = row.get(7)?;
    Ok(AutomationTaskRecord {
        id: row.get(0)?,
        cwd: row.get(1)?,
        kind: row.get(2)?,
        target_thread_id: row.get(3)?,
        title: row.get(4)?,
        prompt: row.get(5)?,
        schedule,
        enabled: enabled != 0,
        execution,
        model: row.get(9)?,
        reasoning_effort: row.get(10)?,
        source_key: row.get(11)?,
        created_at_ms: row.get(12)?,
        updated_at_ms: row.get(13)?,
        last_run_at_ms: row.get(14)?,
        next_run_at_ms: row.get(15)?,
        last_status: row.get(16)?,
        last_error: row.get(17)?,
    })
}

fn automation_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationRunRecord> {
    automation_run_from_row_at(row, 0)
}

fn automation_run_from_row_at(
    row: &rusqlite::Row<'_>,
    offset: usize,
) -> rusqlite::Result<AutomationRunRecord> {
    let metadata_json: Option<String> = row.get(offset + 9)?;
    let metadata = metadata_json
        .as_deref()
        .map(|value| json_from_column(value, offset + 9))
        .transpose()?;
    Ok(AutomationRunRecord {
        id: row.get(offset)?,
        automation_id: row.get(offset + 1)?,
        trigger: row.get(offset + 2)?,
        status: row.get(offset + 3)?,
        started_at_ms: row.get(offset + 4)?,
        completed_at_ms: row.get(offset + 5)?,
        thread_id: row.get(offset + 6)?,
        source_key: row.get(offset + 7)?,
        error: row.get(offset + 8)?,
        metadata,
    })
}

fn json_from_column(value: &str, index: usize) -> rusqlite::Result<Value> {
    serde_json::from_str(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(err))
    })
}

fn bounded_automation_error(value: &str) -> String {
    const MAX: usize = 2_000;
    if value.len() <= MAX {
        return value.to_string();
    }
    let mut end = MAX;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}
