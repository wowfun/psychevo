use psychevo_agent_core::{Message, now_ms};
use rusqlite::{Connection, params, params_from_iter, types::Value as SqlValue};
use serde_json::{Value, json};

use crate::error::Result;
use crate::paths::canonical_cwd;
use crate::types::{
    SessionUsageOptions, SessionUsageSummary, StatsOptions, UsageActivity, UsageActivityDay,
    UsageReadOptions, UsageReadResult, UsageWindowSummary,
};

pub fn usage_stats(options: StatsOptions) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let cutoff_ms = options
        .days
        .map(|days| now_ms().saturating_sub(days as i64 * 86_400_000));
    let scope = StatsScope {
        cwd: (!options.all).then(|| cwd.to_string_lossy().to_string()),
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
                "cwd": scope.cwd,
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
        let context_input_tokens = usage_u64(
            accounting,
            usage,
            "context_input_tokens",
            &["input_tokens", "prompt_tokens", "context_input_tokens"],
        )
        .unwrap_or(0);
        let reasoning_tokens =
            usage_u64(accounting, usage, "reasoning_tokens", &["reasoning_tokens"]).unwrap_or(0);
        let cache_read_tokens = usage_u64(
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
        let cache_write_tokens = usage_u64(
            accounting,
            usage,
            "cache_write_tokens",
            &["cache_write_tokens", "cache_creation_input_tokens"],
        )
        .unwrap_or(0);
        let output_tokens = usage_u64(
            accounting,
            usage,
            "billable_output_tokens",
            &["output_tokens", "completion_tokens"],
        )
        .unwrap_or(0);
        totals.context_input_tokens += context_input_tokens;
        totals.billable_input_tokens += json_u64(accounting, "billable_input_tokens")
            .unwrap_or_else(|| {
                context_input_tokens
                    .saturating_sub(cache_read_tokens)
                    .saturating_sub(cache_write_tokens)
            });
        totals.billable_output_tokens += json_u64(accounting, "billable_output_tokens")
            .unwrap_or_else(|| output_tokens.saturating_sub(reasoning_tokens));
        totals.reasoning_tokens += reasoning_tokens;
        totals.cache_read_tokens += cache_read_tokens;
        totals.cache_write_tokens += cache_write_tokens;
        let reported_total = usage_u64(
            accounting,
            usage,
            "reported_total_tokens",
            &["total_tokens", "reported_total_tokens"],
        )
        .unwrap_or(0);
        totals.reported_total_tokens += reported_total;
        totals.add_cost_status(accounting, is_assistant, reported_total);
        if let Some(cost) = json_i64(accounting, "estimated_cost_nanodollars") {
            totals.estimated_cost_nanodollars += cost;
        }
    }
    let cache_read_percent =
        cache_read_percent(totals.cache_read_tokens, totals.billable_input_tokens);
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
        cost_status: totals.cost_status(),
        estimated_pricing_count: totals.estimated_pricing_count,
        free_pricing_count: totals.free_pricing_count,
        included_pricing_count: totals.included_pricing_count,
        unknown_pricing_count: totals.unknown_pricing_count,
        cache_read_percent,
    })
}

pub fn usage_read(options: UsageReadOptions) -> Result<UsageReadResult> {
    let generated_at_ms = now_ms();
    let activity_days = options.activity_days.clamp(1, 366);
    let window_specs = [
        ("all", "All time", None),
        (
            "30d",
            "Last 30 days",
            Some(generated_at_ms.saturating_sub(30 * 86_400_000)),
        ),
        (
            "7d",
            "Last 7 days",
            Some(generated_at_ms.saturating_sub(7 * 86_400_000)),
        ),
    ];
    options.state.store().with_conn(|conn| {
        let mut windows = Vec::new();
        for (id, label, since_ms) in window_specs {
            windows.push(usage_window_summary(conn, id, label, since_ms)?);
        }
        let activity = usage_activity(conn, generated_at_ms, activity_days)?;
        Ok(UsageReadResult {
            generated_at_ms,
            windows,
            activity,
        })
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
    estimated_pricing_count: u64,
    free_pricing_count: u64,
    included_pricing_count: u64,
    unknown_pricing_count: u64,
}

impl SessionUsageTotals {
    fn add_cost_status(
        &mut self,
        accounting: Option<&Value>,
        is_assistant: bool,
        reported_total_tokens: u64,
    ) {
        if !is_assistant || reported_total_tokens == 0 {
            return;
        }
        match cost_status_from_accounting(accounting).as_str() {
            "estimated" => self.estimated_pricing_count += 1,
            "free" => self.free_pricing_count += 1,
            "included" => self.included_pricing_count += 1,
            _ => self.unknown_pricing_count += 1,
        }
    }

    fn cost_status(&self) -> String {
        aggregate_cost_status(
            self.estimated_pricing_count,
            self.free_pricing_count,
            self.included_pricing_count,
            self.unknown_pricing_count,
        )
    }
}

fn cost_status_from_accounting(accounting: Option<&Value>) -> String {
    if let Some(status) = accounting
        .and_then(|value| value.get("cost_status"))
        .and_then(Value::as_str)
    {
        return match status {
            "estimated" | "free" | "included" | "unknown" => status.to_string(),
            _ => "unknown".to_string(),
        };
    }
    match json_i64(accounting, "estimated_cost_nanodollars") {
        Some(0) => "free".to_string(),
        Some(_) => "estimated".to_string(),
        None => "unknown".to_string(),
    }
}

fn aggregate_cost_status(
    estimated_count: u64,
    free_count: u64,
    included_count: u64,
    unknown_count: u64,
) -> String {
    let known_count = estimated_count + free_count + included_count;
    if unknown_count > 0 && known_count > 0 {
        "mixed".to_string()
    } else if unknown_count > 0 {
        "unknown".to_string()
    } else if estimated_count > 0 {
        "estimated".to_string()
    } else if included_count > 0 {
        "included".to_string()
    } else if free_count > 0 {
        "free".to_string()
    } else {
        "unknown".to_string()
    }
}

fn cache_read_percent(cache_read_tokens: u64, billable_input_tokens: u64) -> Option<f64> {
    let denominator = cache_read_tokens + billable_input_tokens;
    (denominator > 0).then(|| cache_read_tokens as f64 * 100.0 / denominator as f64)
}

fn usage_window_summary(
    conn: &Connection,
    id: &str,
    label: &str,
    since_ms: Option<i64>,
) -> Result<UsageWindowSummary> {
    let where_clause = if since_ms.is_some() {
        "WHERE m.timestamp_ms >= ?1"
    } else {
        "WHERE 1 = 1"
    };
    let mut stmt = conn.prepare(&format!(
        r#"
        SELECT
            COUNT(DISTINCT m.session_id),
            COUNT(m.id),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' THEN 1 ELSE 0 END), 0),
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
                      AND {cost_status_sql} = 'estimated' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant'
                      AND m.reported_total_tokens IS NOT NULL
                      AND {cost_status_sql} = 'free' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant'
                      AND m.reported_total_tokens IS NOT NULL
                      AND {cost_status_sql} = 'included' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant'
                      AND m.reported_total_tokens IS NOT NULL
                      AND {cost_status_sql} = 'unknown' THEN 1 ELSE 0 END), 0)
        FROM messages m
        {where_clause}
        "#,
        cost_status_sql = cost_status_sql(),
    ))?;
    let params = since_ms
        .map(|value| vec![SqlValue::Integer(value)])
        .unwrap_or_default();
    stmt.query_row(params_from_iter(params.iter()), |row| {
        let cache_read_tokens = row_u64(row, 7)?;
        let billable_input_tokens = row_u64(row, 4)?;
        let estimated_pricing_count = row_u64(row, 11)?;
        let free_pricing_count = row_u64(row, 12)?;
        let included_pricing_count = row_u64(row, 13)?;
        let unknown_pricing_count = row_u64(row, 14)?;
        Ok(UsageWindowSummary {
            id: id.to_string(),
            label: label.to_string(),
            since_ms,
            session_count: row_u64(row, 0)?,
            message_count: row_u64(row, 1)?,
            assistant_message_count: row_u64(row, 2)?,
            context_input_tokens: row_u64(row, 3)?,
            billable_input_tokens,
            billable_output_tokens: row_u64(row, 5)?,
            reasoning_tokens: row_u64(row, 6)?,
            cache_read_tokens,
            cache_write_tokens: row_u64(row, 8)?,
            reported_total_tokens: row_u64(row, 9)?,
            estimated_cost_nanodollars: row.get(10)?,
            cost_status: aggregate_cost_status(
                estimated_pricing_count,
                free_pricing_count,
                included_pricing_count,
                unknown_pricing_count,
            ),
            estimated_pricing_count,
            free_pricing_count,
            included_pricing_count,
            unknown_pricing_count,
            cache_read_percent: cache_read_percent(cache_read_tokens, billable_input_tokens),
        })
    })
    .map_err(Into::into)
}

fn usage_activity(
    conn: &Connection,
    generated_at_ms: i64,
    activity_days: usize,
) -> Result<UsageActivity> {
    let start_modifier = format!("-{} day", activity_days.saturating_sub(1));
    let mut stmt = conn.prepare(&format!(
        r#"
        WITH RECURSIVE days(day) AS (
            SELECT date(?1 / 1000, 'unixepoch', 'localtime', ?2)
            UNION ALL
            SELECT date(day, '+1 day') FROM days
            WHERE day < date(?1 / 1000, 'unixepoch', 'localtime')
        ),
        daily AS (
            SELECT
                date(m.timestamp_ms / 1000, 'unixepoch', 'localtime') AS day,
                COUNT(DISTINCT m.session_id) AS session_count,
                COUNT(m.id) AS message_count,
                COALESCE(SUM(m.reported_total_tokens), 0) AS reported_total_tokens,
                COALESCE(SUM(m.context_input_tokens), 0) AS context_input_tokens,
                COALESCE(SUM(m.cache_read_tokens), 0) AS cache_read_tokens,
                COALESCE(SUM(m.cache_write_tokens), 0) AS cache_write_tokens,
                COALESCE(SUM(m.estimated_cost_nanodollars), 0) AS estimated_cost_nanodollars,
                COALESCE(SUM(CASE WHEN m.role = 'assistant'
                          AND m.reported_total_tokens IS NOT NULL
                          AND {cost_status_sql} = 'estimated' THEN 1 ELSE 0 END), 0)
                    AS estimated_pricing_count,
                COALESCE(SUM(CASE WHEN m.role = 'assistant'
                          AND m.reported_total_tokens IS NOT NULL
                          AND {cost_status_sql} = 'free' THEN 1 ELSE 0 END), 0)
                    AS free_pricing_count,
                COALESCE(SUM(CASE WHEN m.role = 'assistant'
                          AND m.reported_total_tokens IS NOT NULL
                          AND {cost_status_sql} = 'included' THEN 1 ELSE 0 END), 0)
                    AS included_pricing_count,
                COALESCE(SUM(CASE WHEN m.role = 'assistant'
                          AND m.reported_total_tokens IS NOT NULL
                          AND {cost_status_sql} = 'unknown' THEN 1 ELSE 0 END), 0)
                    AS unknown_pricing_count
            FROM messages m
            WHERE date(m.timestamp_ms / 1000, 'unixepoch', 'localtime')
                BETWEEN date(?1 / 1000, 'unixepoch', 'localtime', ?2)
                    AND date(?1 / 1000, 'unixepoch', 'localtime')
            GROUP BY day
        )
        SELECT
            days.day,
            COALESCE(daily.session_count, 0),
            COALESCE(daily.message_count, 0),
            COALESCE(daily.reported_total_tokens, 0),
            COALESCE(daily.context_input_tokens, 0),
            COALESCE(daily.cache_read_tokens, 0),
            COALESCE(daily.cache_write_tokens, 0),
            COALESCE(daily.estimated_cost_nanodollars, 0),
            COALESCE(daily.estimated_pricing_count, 0),
            COALESCE(daily.free_pricing_count, 0),
            COALESCE(daily.included_pricing_count, 0),
            COALESCE(daily.unknown_pricing_count, 0)
        FROM days
        LEFT JOIN daily ON daily.day = days.day
        ORDER BY days.day ASC
        "#,
        cost_status_sql = cost_status_sql(),
    ))?;
    let rows = stmt.query_map(params![generated_at_ms, start_modifier], |row| {
        let estimated_pricing_count = row_u64(row, 8)?;
        let free_pricing_count = row_u64(row, 9)?;
        let included_pricing_count = row_u64(row, 10)?;
        let unknown_pricing_count = row_u64(row, 11)?;
        Ok(UsageActivityDay {
            date: row.get(0)?,
            session_count: row_u64(row, 1)?,
            message_count: row_u64(row, 2)?,
            reported_total_tokens: row_u64(row, 3)?,
            context_input_tokens: row_u64(row, 4)?,
            cache_read_tokens: row_u64(row, 5)?,
            cache_write_tokens: row_u64(row, 6)?,
            estimated_cost_nanodollars: row.get(7)?,
            cost_status: aggregate_cost_status(
                estimated_pricing_count,
                free_pricing_count,
                included_pricing_count,
                unknown_pricing_count,
            ),
            estimated_pricing_count,
            free_pricing_count,
            included_pricing_count,
            unknown_pricing_count,
        })
    })?;
    let mut days = Vec::new();
    for row in rows {
        days.push(row?);
    }
    let start_date = days.first().map(|day| day.date.clone()).unwrap_or_default();
    let end_date = days.last().map(|day| day.date.clone()).unwrap_or_default();
    Ok(UsageActivity {
        start_date,
        end_date,
        days,
    })
}

fn cost_status_sql() -> &'static str {
    r#"
    COALESCE(
        m.cost_status,
        CASE
            WHEN m.estimated_cost_nanodollars IS NULL THEN 'unknown'
            WHEN m.estimated_cost_nanodollars = 0 THEN 'free'
            ELSE 'estimated'
        END
    )
    "#
}

fn row_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    row.get::<_, i64>(index).map(|value| value.max(0) as u64)
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
    pub(crate) cwd: Option<String>,
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
                      AND {cost_status_sql} = 'estimated' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant'
                      AND m.reported_total_tokens IS NOT NULL
                      AND {cost_status_sql} = 'free' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant'
                      AND m.reported_total_tokens IS NOT NULL
                      AND {cost_status_sql} = 'included' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant'
                      AND m.reported_total_tokens IS NOT NULL
                      AND {cost_status_sql} = 'unknown' THEN 1 ELSE 0 END), 0)
        FROM sessions s
        LEFT JOIN messages m ON m.session_id = s.id
        {}
        "#,
        scope_where_clause(scope),
        cost_status_sql = cost_status_sql(),
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
            "estimated_priced_messages": row.get::<_, i64>(10)?,
            "free_priced_messages": row.get::<_, i64>(11)?,
            "included_priced_messages": row.get::<_, i64>(12)?,
            "unknown_priced_messages": row.get::<_, i64>(13)?,
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
            s.cwd,
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
            "cwd": row.get::<_, String>(2)?,
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
    match (scope.cwd.is_some(), scope.cutoff_ms.is_some()) {
        (false, false) => "WHERE 1 = 1",
        (true, false) => "WHERE s.cwd = ?1",
        (false, true) => "WHERE s.updated_at_ms >= ?1",
        (true, true) => "WHERE s.cwd = ?1 AND s.updated_at_ms >= ?2",
    }
}

pub(crate) fn scope_params(scope: &StatsScope) -> Vec<SqlValue> {
    let mut values = Vec::new();
    if let Some(cwd) = &scope.cwd {
        values.push(SqlValue::Text(cwd.clone()));
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
