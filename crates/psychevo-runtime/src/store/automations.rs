use psychevo_agent_core::now_ms;
use sqlx::Row;
use uuid::Uuid;

use crate::error::{Error, Result};

use super::{
    AutomationRunFinishInput, AutomationRunRecord, AutomationRunRecoveryCandidate,
    AutomationTaskInput, AutomationTaskRecord, StateRuntime,
};

impl StateRuntime {
    pub async fn upsert_automation_task(
        &self,
        input: AutomationTaskInput,
    ) -> Result<AutomationTaskRecord> {
        let id = input.id.unwrap_or_else(|| Uuid::now_v7().to_string());
        let now = now_ms();
        let schedule_json = serde_json::to_string(&input.schedule)?;
        let execution_json = serde_json::to_string(&input.execution)?;
        self.automation_write(
            sqlx::query(
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
            )
            .bind(&id)
            .bind(input.cwd)
            .bind(input.kind)
            .bind(input.target_thread_id)
            .bind(input.title)
            .bind(input.prompt)
            .bind(schedule_json)
            .bind(i64::from(input.enabled))
            .bind(execution_json)
            .bind(input.model)
            .bind(input.reasoning_effort)
            .bind(input.source_key)
            .bind(now)
            .bind(input.next_run_at_ms),
        )
        .await?;
        self.automation_task(&id)
            .await?
            .ok_or_else(|| Error::Message(format!("automation task not found after upsert: {id}")))
    }

    pub async fn automation_task(&self, id: &str) -> Result<Option<AutomationTaskRecord>> {
        let sql = automation_task_select_sql("WHERE id = ?1");
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(id)
                .fetch_optional(&mut *conn)
                .await?
                .map(|row| automation_task_from_row(&row))
                .transpose()
        })
        .await
    }

    pub async fn automation_tasks_for_cwd(&self, cwd: &str) -> Result<Vec<AutomationTaskRecord>> {
        self.automation_tasks_for_optional_cwd(Some(cwd)).await
    }

    pub async fn automation_tasks_for_optional_cwd(
        &self,
        cwd: Option<&str>,
    ) -> Result<Vec<AutomationTaskRecord>> {
        let sql = automation_task_select_sql(
            "WHERE (?1 IS NULL OR cwd = ?1)
             ORDER BY enabled DESC, next_run_at_ms IS NULL, next_run_at_ms ASC, updated_at_ms DESC",
        );
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(cwd)
                .fetch_all(&mut *conn)
                .await?;
            rows.into_iter()
                .map(|row| automation_task_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn due_automation_tasks(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<AutomationTaskRecord>> {
        let sql = automation_task_select_sql(
            "WHERE enabled = 1 AND next_run_at_ms IS NOT NULL AND next_run_at_ms <= ?1
             ORDER BY next_run_at_ms ASC, updated_at_ms ASC
             LIMIT ?2",
        );
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(now_ms)
                .bind(limit as i64)
                .fetch_all(&mut *conn)
                .await?;
            rows.into_iter()
                .map(|row| automation_task_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn delete_automation_task(&self, id: &str) -> Result<bool> {
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let changed = sqlx::query("DELETE FROM automations WHERE id = ?1")
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(changed > 0)
        })
        .await
    }

    pub async fn claim_automation_run(
        &self,
        automation_id: &str,
        trigger: &str,
    ) -> Result<Option<AutomationRunRecord>> {
        let id = Uuid::now_v7().to_string();
        let now = now_ms();
        let inserted = self
            .observe_sqlx(async {
                let mut tx = self.begin_sqlx_write().await?;
                let running = sqlx::query_scalar::<_, String>(
                    "SELECT id FROM automation_runs WHERE automation_id = ?1 AND status = 'running' LIMIT 1",
                )
                .bind(automation_id)
                .fetch_optional(&mut *tx)
                .await?;
                if running.is_some() {
                    return Ok(false);
                }
                sqlx::query(
                    r#"
                    INSERT INTO automation_runs (
                        id, automation_id, trigger, status, started_at_ms
                    ) VALUES (?1, ?2, ?3, 'running', ?4)
                    "#,
                )
                .bind(&id)
                .bind(automation_id)
                .bind(trigger)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    r#"
                    UPDATE automations
                    SET last_run_at_ms = ?2,
                        last_status = 'running',
                        last_error = NULL,
                        updated_at_ms = ?2
                    WHERE id = ?1
                    "#,
                )
                .bind(automation_id)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                self.finish_sqlx_write().await;
                Ok(true)
            })
            .await?;
        if !inserted {
            return Ok(None);
        }
        self.automation_run(&id).await
    }

    pub async fn automation_run(&self, id: &str) -> Result<Option<AutomationRunRecord>> {
        let sql = automation_run_select_sql("WHERE id = ?1");
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(id)
                .fetch_optional(&mut *conn)
                .await?
                .map(|row| automation_run_from_row(&row))
                .transpose()
        })
        .await
    }

    pub async fn automation_runs_for_task(
        &self,
        automation_id: &str,
        limit: usize,
    ) -> Result<Vec<AutomationRunRecord>> {
        let sql = automation_run_select_sql(
            "WHERE automation_id = ?1 ORDER BY started_at_ms DESC LIMIT ?2",
        );
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(automation_id)
                .bind(limit as i64)
                .fetch_all(&mut *conn)
                .await?;
            rows.into_iter()
                .map(|row| automation_run_from_row(&row))
                .collect()
        })
        .await
    }

    pub async fn stale_automation_runs_for_recovery(
        &self,
        now_ms: i64,
        stale_after_ms: i64,
        limit: usize,
    ) -> Result<Vec<AutomationRunRecoveryCandidate>> {
        let stale_before_ms = now_ms.saturating_sub(stale_after_ms);
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let rows = sqlx::query(
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
            )
            .bind(stale_before_ms)
            .bind(now_ms)
            .bind(limit as i64)
            .fetch_all(&mut *conn)
            .await?;
            rows.into_iter()
                .map(|row| {
                    Ok(AutomationRunRecoveryCandidate {
                        task: automation_task_from_row(&row)?,
                        run: automation_run_from_row_at(&row, 18)?,
                    })
                })
                .collect()
        })
        .await
    }

    pub async fn finish_automation_run(
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
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let automation_id = sqlx::query_scalar::<_, String>(
                "SELECT automation_id FROM automation_runs WHERE id = ?1",
            )
            .bind(input.run_id)
            .fetch_optional(&mut *tx)
            .await?;
            let Some(automation_id) = automation_id else {
                return Ok(false);
            };
            let changed = sqlx::query(
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
            )
            .bind(input.run_id)
            .bind(input.status)
            .bind(now)
            .bind(input.thread_id)
            .bind(input.source_key)
            .bind(error.as_deref())
            .bind(metadata_json)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed == 0 {
                return Ok(false);
            }
            sqlx::query(
                r#"
                UPDATE automations
                SET last_status = ?2,
                    last_error = ?3,
                    next_run_at_ms = ?4,
                    source_key = COALESCE(?5, source_key),
                    updated_at_ms = ?6
                WHERE id = ?1
                "#,
            )
            .bind(automation_id)
            .bind(input.status)
            .bind(error.as_deref())
            .bind(input.next_run_at_ms)
            .bind(input.source_key)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            self.finish_sqlx_write().await;
            Ok(true)
        })
        .await?;
        self.automation_run(input.run_id).await
    }

    async fn automation_write<'q>(
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

fn automation_task_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<AutomationTaskRecord> {
    let schedule_json: String = row.try_get(6)?;
    let execution_json: String = row.try_get(8)?;
    let enabled: i64 = row.try_get(7)?;
    Ok(AutomationTaskRecord {
        id: row.try_get(0)?,
        cwd: row.try_get(1)?,
        kind: row.try_get(2)?,
        target_thread_id: row.try_get(3)?,
        title: row.try_get(4)?,
        prompt: row.try_get(5)?,
        schedule: serde_json::from_str(&schedule_json)?,
        enabled: enabled != 0,
        execution: serde_json::from_str(&execution_json)?,
        model: row.try_get(9)?,
        reasoning_effort: row.try_get(10)?,
        source_key: row.try_get(11)?,
        created_at_ms: row.try_get(12)?,
        updated_at_ms: row.try_get(13)?,
        last_run_at_ms: row.try_get(14)?,
        next_run_at_ms: row.try_get(15)?,
        last_status: row.try_get(16)?,
        last_error: row.try_get(17)?,
    })
}

fn automation_run_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<AutomationRunRecord> {
    automation_run_from_row_at(row, 0)
}

fn automation_run_from_row_at(
    row: &sqlx::sqlite::SqliteRow,
    offset: usize,
) -> Result<AutomationRunRecord> {
    let metadata_json: Option<String> = row.try_get(offset + 9)?;
    let metadata = metadata_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    Ok(AutomationRunRecord {
        id: row.try_get(offset)?,
        automation_id: row.try_get(offset + 1)?,
        trigger: row.try_get(offset + 2)?,
        status: row.try_get(offset + 3)?,
        started_at_ms: row.try_get(offset + 4)?,
        completed_at_ms: row.try_get(offset + 5)?,
        thread_id: row.try_get(offset + 6)?,
        source_key: row.try_get(offset + 7)?,
        error: row.try_get(offset + 8)?,
        metadata,
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
