use psychevo_agent_core::now_ms;
use rusqlite::{Connection, params_from_iter, types::Value as SqlValue};
use serde_json::{Value, json};

use crate::error::Result;
use crate::paths::canonical_workdir;
use crate::types::StatsOptions;

pub fn usage_stats(options: StatsOptions) -> Result<Value> {
    let workdir = canonical_workdir(&options.workdir)?;
    let cutoff_ms = options
        .days
        .map(|days| now_ms().saturating_sub(days as i64 * 86_400_000));
    let scope = StatsScope {
        workdir: (!options.all).then(|| workdir.to_string_lossy().to_string()),
        cutoff_ms,
        limit: options.limit.max(1),
    };
    options.state.store().with_conn(|conn| {
        let totals = totals(conn, &scope)?;
        let provider_models = provider_models(conn, &scope)?;
        let top_tools = top_tools(conn, &scope)?;
        let top_sessions = top_sessions(conn, &scope)?;
        Ok(json!({
            "scope": {
                "all": options.all,
                "workdir": scope.workdir,
                "days": options.days,
            },
            "totals": totals,
            "provider_models": provider_models,
            "top_tools": top_tools,
            "top_sessions": top_sessions,
        }))
    })
}

pub(crate) struct StatsScope {
    pub(crate) workdir: Option<String>,
    pub(crate) cutoff_ms: Option<i64>,
    pub(crate) limit: usize,
}

pub(crate) fn totals(conn: &Connection, scope: &StatsScope) -> Result<Value> {
    let mut stmt = conn.prepare(&format!(
        r#"
        SELECT
            COUNT(DISTINCT s.id),
            COUNT(m.id),
            COALESCE(SUM(m.context_input_tokens), 0),
            COALESCE(SUM(m.billable_input_tokens), 0),
            COALESCE(SUM(m.billable_output_tokens), 0),
            COALESCE(SUM(m.reasoning_tokens), 0),
            COALESCE(SUM(m.cache_read_tokens), 0),
            COALESCE(SUM(m.cache_write_tokens), 0),
            COALESCE(SUM(m.reported_total_tokens), 0),
            COALESCE(SUM(m.estimated_cost_nanodollars), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant'
                      AND m.reported_total_tokens IS NOT NULL
                      AND m.pricing_source IS NULL THEN 1 ELSE 0 END), 0)
        FROM sessions s
        LEFT JOIN messages m ON m.session_id = s.id
        {}
        "#,
        scope_where_clause(scope)
    ))?;
    let params = scope_params(scope);
    let values = stmt.query_row(params_from_iter(params.iter()), |row| {
        Ok(json!({
            "sessions": row.get::<_, i64>(0)?,
            "messages": row.get::<_, i64>(1)?,
            "context_input_tokens": row.get::<_, i64>(2)?,
            "billable_input_tokens": row.get::<_, i64>(3)?,
            "billable_output_tokens": row.get::<_, i64>(4)?,
            "reasoning_tokens": row.get::<_, i64>(5)?,
            "cache_read_tokens": row.get::<_, i64>(6)?,
            "cache_write_tokens": row.get::<_, i64>(7)?,
            "reported_total_tokens": row.get::<_, i64>(8)?,
            "estimated_cost_nanodollars": row.get::<_, i64>(9)?,
            "unknown_priced_messages": row.get::<_, i64>(10)?,
        }))
    })?;
    Ok(values)
}

pub(crate) fn provider_models(conn: &Connection, scope: &StatsScope) -> Result<Value> {
    let mut stmt = conn.prepare(&format!(
        r#"
        SELECT
            COALESCE(m.provider, s.provider),
            COALESCE(m.model, s.model),
            COUNT(*),
            COALESCE(SUM(m.reported_total_tokens), 0),
            COALESCE(SUM(m.estimated_cost_nanodollars), 0)
        FROM messages m
        JOIN sessions s ON s.id = m.session_id
        {}
          AND m.role = 'assistant'
        GROUP BY COALESCE(m.provider, s.provider), COALESCE(m.model, s.model)
        ORDER BY COALESCE(SUM(m.estimated_cost_nanodollars), 0) DESC,
                 COALESCE(SUM(m.reported_total_tokens), 0) DESC
        LIMIT ?{}
        "#,
        scope_where_clause(scope),
        scope_params(scope).len() + 1
    ))?;
    let mut params = scope_params(scope);
    params.push(SqlValue::Integer(scope.limit as i64));
    let rows = stmt.query_map(params_from_iter(params.iter()), |row| {
        Ok(json!({
            "provider": row.get::<_, String>(0)?,
            "model": row.get::<_, String>(1)?,
            "messages": row.get::<_, i64>(2)?,
            "reported_total_tokens": row.get::<_, i64>(3)?,
            "estimated_cost_nanodollars": row.get::<_, i64>(4)?,
        }))
    })?;
    collect_json_rows(rows)
}

pub(crate) fn top_tools(conn: &Connection, scope: &StatsScope) -> Result<Value> {
    let mut stmt = conn.prepare(&format!(
        r#"
        SELECT m.tool_name, COUNT(*)
        FROM messages m
        JOIN sessions s ON s.id = m.session_id
        {}
          AND m.role = 'tool_result'
          AND m.tool_name IS NOT NULL
        GROUP BY m.tool_name
        ORDER BY COUNT(*) DESC, m.tool_name ASC
        LIMIT ?{}
        "#,
        scope_where_clause(scope),
        scope_params(scope).len() + 1
    ))?;
    let mut params = scope_params(scope);
    params.push(SqlValue::Integer(scope.limit as i64));
    let rows = stmt.query_map(params_from_iter(params.iter()), |row| {
        Ok(json!({
            "tool": row.get::<_, String>(0)?,
            "calls": row.get::<_, i64>(1)?,
        }))
    })?;
    collect_json_rows(rows)
}

pub(crate) fn top_sessions(conn: &Connection, scope: &StatsScope) -> Result<Value> {
    let mut stmt = conn.prepare(&format!(
        r#"
        SELECT
            s.id,
            s.title,
            s.workdir,
            s.provider,
            s.model,
            COALESCE(SUM(m.reported_total_tokens), 0),
            COALESCE(SUM(m.estimated_cost_nanodollars), 0),
            s.updated_at_ms
        FROM sessions s
        LEFT JOIN messages m ON m.session_id = s.id
        {}
        GROUP BY s.id
        ORDER BY COALESCE(SUM(m.estimated_cost_nanodollars), 0) DESC,
                 COALESCE(SUM(m.reported_total_tokens), 0) DESC,
                 s.updated_at_ms DESC
        LIMIT ?{}
        "#,
        scope_where_clause(scope),
        scope_params(scope).len() + 1
    ))?;
    let mut params = scope_params(scope);
    params.push(SqlValue::Integer(scope.limit as i64));
    let rows = stmt.query_map(params_from_iter(params.iter()), |row| {
        Ok(json!({
            "session": row.get::<_, String>(0)?,
            "title": row.get::<_, Option<String>>(1)?,
            "workdir": row.get::<_, String>(2)?,
            "provider": row.get::<_, String>(3)?,
            "model": row.get::<_, String>(4)?,
            "reported_total_tokens": row.get::<_, i64>(5)?,
            "estimated_cost_nanodollars": row.get::<_, i64>(6)?,
            "updated_at_ms": row.get::<_, i64>(7)?,
        }))
    })?;
    collect_json_rows(rows)
}

pub(crate) fn scope_where_clause(scope: &StatsScope) -> &'static str {
    match (scope.workdir.is_some(), scope.cutoff_ms.is_some()) {
        (false, false) => "WHERE 1 = 1",
        (true, false) => "WHERE s.workdir = ?1",
        (false, true) => "WHERE s.updated_at_ms >= ?1",
        (true, true) => "WHERE s.workdir = ?1 AND s.updated_at_ms >= ?2",
    }
}

pub(crate) fn scope_params(scope: &StatsScope) -> Vec<SqlValue> {
    let mut values = Vec::new();
    if let Some(workdir) = &scope.workdir {
        values.push(SqlValue::Text(workdir.clone()));
    }
    if let Some(cutoff_ms) = &scope.cutoff_ms {
        values.push(SqlValue::Integer(*cutoff_ms));
    }
    values
}

pub(crate) fn collect_json_rows<F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Value>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<Value>,
{
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(Value::Array(values))
}
