use psychevo_agent_core::{Message, now_ms};
use rusqlite::{Connection, params_from_iter, types::Value as SqlValue};
use serde_json::{Value, json};

use crate::error::Result;
use crate::paths::canonical_workdir;
use crate::types::{SessionUsageOptions, SessionUsageSummary, StatsOptions};

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

pub fn session_usage_summary(options: SessionUsageOptions) -> Result<SessionUsageSummary> {
    let store = options.state.store();
    let summary = store.session_summary(&options.session_id)?.ok_or_else(|| {
        crate::Error::Message(format!("session not found: {}", options.session_id))
    })?;
    let messages = store.load_tui_message_summaries(&summary.id)?;
    let mut totals = SessionUsageTotals::default();
    let mut provider = summary.provider;
    let mut model = summary.model;
    for message in messages {
        totals.message_count += 1;
        let is_assistant = matches!(message.message, Message::Assistant { .. });
        if is_assistant {
            totals.assistant_message_count += 1;
            if let Message::Assistant {
                provider: message_provider,
                model: message_model,
                ..
            } = &message.message
            {
                if let Some(value) = message_provider {
                    provider = value.clone();
                }
                if let Some(value) = message_model {
                    model = value.clone();
                }
            }
        }
        let accounting = message.accounting.as_ref();
        let usage = message.usage.as_ref();
        totals.context_input_tokens += usage_u64(
            accounting,
            usage,
            "context_input_tokens",
            &["input_tokens", "prompt_tokens", "context_input_tokens"],
        )
        .unwrap_or(0);
        totals.billable_input_tokens += json_u64(accounting, "billable_input_tokens").unwrap_or(0);
        totals.billable_output_tokens +=
            json_u64(accounting, "billable_output_tokens").unwrap_or(0);
        totals.reasoning_tokens +=
            usage_u64(accounting, usage, "reasoning_tokens", &["reasoning_tokens"]).unwrap_or(0);
        totals.cache_read_tokens += usage_u64(
            accounting,
            usage,
            "cache_read_tokens",
            &[
                "cached_tokens",
                "cached_input_tokens",
                "cache_read_tokens",
                "cache_read_input_tokens",
            ],
        )
        .unwrap_or(0);
        totals.cache_write_tokens += usage_u64(
            accounting,
            usage,
            "cache_write_tokens",
            &["cache_write_tokens", "cache_creation_input_tokens"],
        )
        .unwrap_or(0);
        let reported_total = usage_u64(
            accounting,
            usage,
            "reported_total_tokens",
            &["total_tokens", "reported_total_tokens"],
        )
        .unwrap_or(0);
        totals.reported_total_tokens += reported_total;
        totals.estimated_cost_nanodollars +=
            json_i64(accounting, "estimated_cost_nanodollars").unwrap_or(0);
        if is_assistant
            && reported_total > 0
            && accounting
                .and_then(|value| value.get("pricing_source"))
                .map(Value::is_null)
                .unwrap_or(true)
        {
            totals.unknown_pricing_count += 1;
        }
    }
    let cache_read_percent = (totals.context_input_tokens > 0)
        .then(|| totals.cache_read_tokens as f64 * 100.0 / totals.context_input_tokens as f64);
    Ok(SessionUsageSummary {
        session_id: summary.id,
        provider,
        model,
        message_count: totals.message_count,
        assistant_message_count: totals.assistant_message_count,
        context_input_tokens: totals.context_input_tokens,
        billable_input_tokens: totals.billable_input_tokens,
        billable_output_tokens: totals.billable_output_tokens,
        reasoning_tokens: totals.reasoning_tokens,
        cache_read_tokens: totals.cache_read_tokens,
        cache_write_tokens: totals.cache_write_tokens,
        reported_total_tokens: totals.reported_total_tokens,
        estimated_cost_nanodollars: totals.estimated_cost_nanodollars,
        unknown_pricing_count: totals.unknown_pricing_count,
        cache_read_percent,
    })
}

#[derive(Default)]
struct SessionUsageTotals {
    message_count: u64,
    assistant_message_count: u64,
    context_input_tokens: u64,
    billable_input_tokens: u64,
    billable_output_tokens: u64,
    reasoning_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    reported_total_tokens: u64,
    estimated_cost_nanodollars: i64,
    unknown_pricing_count: u64,
}

fn usage_u64(
    accounting: Option<&Value>,
    usage: Option<&Value>,
    accounting_key: &str,
    usage_keys: &[&str],
) -> Option<u64> {
    json_u64(accounting, accounting_key)
        .or_else(|| usage_keys.iter().find_map(|key| json_u64(usage, key)))
}

fn json_u64(value: Option<&Value>, key: &str) -> Option<u64> {
    value?.get(key).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
    })
}

fn json_i64(value: Option<&Value>, key: &str) -> Option<i64> {
    value?.get(key).and_then(Value::as_i64)
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
