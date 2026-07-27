use psychevo_agent_core::now_ms;
use serde_json::{Value, json};
use sqlx::Row;

use crate::error::Result;
use crate::paths::canonical_cwd;
use crate::types::{
    SessionUsageOptions, SessionUsageSummary, StatsOptions, UsageActivity, UsageActivityDay,
    UsageReadOptions, UsageReadResult, UsageWindowSummary,
};

pub async fn usage_stats(options: StatsOptions) -> Result<Value> {
    let cwd = canonical_cwd(&options.cwd)?;
    let cutoff_ms = options
        .days
        .map(|days| now_ms().saturating_sub(days as i64 * 86_400_000));
    let scope = StatsScope {
        cwd: (!options.all).then(|| cwd.to_string_lossy().to_string()),
        cutoff_ms,
        limit: options.limit.max(1),
    };
    let state = options.state;
    state
        .observe_sqlx(async {
            let mut conn = state.acquire_sqlx().await?;
            let totals = totals(&mut conn, &scope).await?;
            let provider_models = provider_models(&mut conn, &scope).await?;
            let top_tools = top_tools(&mut conn, &scope).await?;
            let top_sessions = top_sessions(&mut conn, &scope).await?;
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
        .await
}

pub async fn session_usage_summary(options: SessionUsageOptions) -> Result<SessionUsageSummary> {
    let store = options.state;
    let boundary = store
        .session_revert_state(&options.session_id)
        .await?
        .map(|revert| revert.start_seq)
        .unwrap_or(i64::MAX);
    let reported_sql = usage_reported_total_sql();
    let input_sql = usage_context_input_sql();
    let output_sql = usage_output_sql();
    let reasoning_sql = usage_reasoning_sql();
    let cache_read_sql = usage_cache_read_sql();
    let cache_write_sql = usage_cache_write_sql();
    let billable_input_sql = format!(
        "COALESCE(m.billable_input_tokens, MAX(COALESCE(({input_sql}), 0) - COALESCE(({cache_read_sql}), 0) - COALESCE(({cache_write_sql}), 0), 0))"
    );
    let billable_output_sql = format!(
        "COALESCE(m.billable_output_tokens, MAX(COALESCE(({output_sql}), 0) - COALESCE(({reasoning_sql}), 0), 0))"
    );
    let complete_sql = format!(
        "(({reported_sql}) IS NOT NULL OR (({input_sql}) IS NOT NULL AND ({output_sql}) IS NOT NULL))"
    );
    let known_sql = format!(
        "(({reported_sql}) IS NOT NULL OR ({input_sql}) IS NOT NULL OR ({output_sql}) IS NOT NULL)"
    );
    let effective_sql = format!(
        "CASE WHEN ({reported_sql}) IS NOT NULL THEN ({reported_sql}) WHEN ({input_sql}) IS NOT NULL OR ({output_sql}) IS NOT NULL THEN COALESCE(({input_sql}), 0) + COALESCE(({output_sql}), 0) ELSE NULL END"
    );
    let sql = format!(
        r#"
        SELECT
            COALESCE((
                SELECT latest.provider
                FROM messages latest
                WHERE latest.session_id = s.id
                  AND latest.session_seq < ?2
                  AND latest.role = 'assistant'
                  AND latest.provider IS NOT NULL
                ORDER BY latest.session_seq DESC
                LIMIT 1
            ), s.provider),
            COALESCE((
                SELECT latest.model
                FROM messages latest
                WHERE latest.session_id = s.id
                  AND latest.session_seq < ?2
                  AND latest.role = 'assistant'
                  AND latest.model IS NOT NULL
                ORDER BY latest.session_seq DESC
                LIMIT 1
            ), s.model),
            COUNT(m.id),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM({input_sql}), 0),
            COALESCE(SUM({billable_input_sql}), 0),
            COALESCE(SUM({billable_output_sql}), 0),
            COALESCE(SUM({reasoning_sql}), 0),
            COALESCE(SUM({cache_read_sql}), 0),
            COALESCE(SUM({cache_write_sql}), 0),
            COALESCE(SUM({reported_sql}), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' THEN {effective_sql} ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND {complete_sql} THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND NOT {complete_sql} THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND ({reported_sql}) IS NULL
                      AND {complete_sql} THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND NOT {complete_sql}
                      AND {known_sql} THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(m.estimated_cost_nanodollars), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND {known_sql}
                      AND {cost_status_sql} = 'estimated' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND {known_sql}
                      AND {cost_status_sql} = 'free' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND {known_sql}
                      AND {cost_status_sql} = 'included' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND {known_sql}
                      AND {cost_status_sql} = 'unknown' THEN 1 ELSE 0 END), 0)
        FROM sessions s
        LEFT JOIN messages m
          ON m.session_id = s.id
         AND m.session_seq < ?2
        WHERE s.id = ?1
        GROUP BY s.id
        "#,
        cost_status_sql = cost_status_sql(),
    );
    let session_id = options.session_id;
    let row = store
        .observe_sqlx(async {
            let mut conn = store.acquire_sqlx().await?;
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&session_id)
                .bind(boundary)
                .fetch_optional(&mut *conn)
                .await?
                .ok_or_else(|| {
                    crate::Error::Message(format!("session not found: {session_id}"))
                })
        })
        .await?;
    let accounted_provider_call_count = row_u64(&row, 12)?;
    let unaccounted_provider_call_count = row_u64(&row, 13)?;
    let derived_provider_call_count = row_u64(&row, 14)?;
    let partial_provider_call_count = row_u64(&row, 15)?;
    let effective_total_tokens = row_u64(&row, 11)?;
    let estimated_pricing_count = row_u64(&row, 17)?;
    let free_pricing_count = row_u64(&row, 18)?;
    let included_pricing_count = row_u64(&row, 19)?;
    let unknown_pricing_count = row_u64(&row, 20)?;
    let context_input_tokens = row_u64(&row, 4)?;
    let cache_read_tokens = row_u64(&row, 8)?;
    Ok(SessionUsageSummary {
        session_id,
        provider: row.try_get(0)?,
        model: row.try_get(1)?,
        message_count: row_u64(&row, 2)?,
        assistant_message_count: row_u64(&row, 3)?,
        context_input_tokens,
        billable_input_tokens: row_u64(&row, 5)?,
        billable_output_tokens: row_u64(&row, 6)?,
        reasoning_tokens: row_u64(&row, 7)?,
        cache_read_tokens,
        cache_write_tokens: row_u64(&row, 9)?,
        effective_total_tokens: (accounted_provider_call_count + partial_provider_call_count > 0)
            .then_some(effective_total_tokens),
        reported_total_tokens: row_u64(&row, 10)?,
        total_status: aggregate_usage_total_status(
            accounted_provider_call_count,
            unaccounted_provider_call_count,
            derived_provider_call_count,
            partial_provider_call_count,
        )
        .to_string(),
        accounted_provider_call_count,
        unaccounted_provider_call_count,
        estimated_cost_nanodollars: row.try_get(16)?,
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
        cache_read_percent: cache_read_percent(cache_read_tokens, context_input_tokens),
    })
}

pub async fn usage_read(options: UsageReadOptions) -> Result<UsageReadResult> {
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
    let state = options.state;
    state
        .observe_sqlx(async {
            let mut conn = state.acquire_sqlx().await?;
            let mut windows = Vec::new();
            for (id, label, since_ms) in window_specs {
                windows.push(usage_window_summary(&mut conn, id, label, since_ms).await?);
            }
            let activity = usage_activity(&mut conn, generated_at_ms, activity_days).await?;
            Ok(UsageReadResult {
                generated_at_ms,
                windows,
                activity,
            })
        })
        .await
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

fn cache_read_percent(cache_read_tokens: u64, context_input_tokens: u64) -> Option<f64> {
    (context_input_tokens > 0)
        .then(|| cache_read_tokens as f64 * 100.0 / context_input_tokens as f64)
}

fn aggregate_usage_total_status(
    accounted_provider_call_count: u64,
    unaccounted_provider_call_count: u64,
    derived_provider_call_count: u64,
    partial_provider_call_count: u64,
) -> &'static str {
    if unaccounted_provider_call_count > 0 {
        if accounted_provider_call_count > 0 || partial_provider_call_count > 0 {
            "partial"
        } else {
            "unavailable"
        }
    } else if accounted_provider_call_count == 0 {
        "unavailable"
    } else if derived_provider_call_count > 0 {
        "derived"
    } else {
        "reported"
    }
}

fn usage_reported_total_sql() -> &'static str {
    "COALESCE(m.reported_total_tokens, json_extract(m.usage_json, '$.total_tokens'), json_extract(m.usage_json, '$.reported_total_tokens'), json_extract(m.usage_json, '$.totalTokens'))"
}

fn usage_context_input_sql() -> &'static str {
    "COALESCE(m.context_input_tokens, json_extract(m.usage_json, '$.input_tokens'), json_extract(m.usage_json, '$.prompt_tokens'), json_extract(m.usage_json, '$.context_input_tokens'), json_extract(m.usage_json, '$.inputTokens'))"
}

fn usage_output_sql() -> &'static str {
    "COALESCE(json_extract(m.usage_json, '$.output_tokens'), json_extract(m.usage_json, '$.completion_tokens'), json_extract(m.usage_json, '$.outputTokens'), CASE WHEN m.billable_output_tokens IS NOT NULL THEN m.billable_output_tokens + COALESCE(m.reasoning_tokens, 0) END)"
}

fn usage_reasoning_sql() -> &'static str {
    "COALESCE(m.reasoning_tokens, json_extract(m.usage_json, '$.reasoning_tokens'))"
}

fn usage_cache_read_sql() -> &'static str {
    "COALESCE(m.cache_read_tokens, json_extract(m.usage_json, '$.cached_tokens'), json_extract(m.usage_json, '$.cached_input_tokens'), json_extract(m.usage_json, '$.cache_read_tokens'), json_extract(m.usage_json, '$.cache_read_input_tokens'))"
}

fn usage_cache_write_sql() -> &'static str {
    "COALESCE(m.cache_write_tokens, json_extract(m.usage_json, '$.cache_write_tokens'), json_extract(m.usage_json, '$.cache_creation_input_tokens'))"
}

async fn usage_window_summary(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    id: &str,
    label: &str,
    since_ms: Option<i64>,
) -> Result<UsageWindowSummary> {
    let where_clause = if since_ms.is_some() {
        "WHERE m.timestamp_ms >= ?1"
    } else {
        "WHERE 1 = 1"
    };
    let reported_sql = usage_reported_total_sql();
    let input_sql = usage_context_input_sql();
    let output_sql = usage_output_sql();
    let complete_sql = format!(
        "(({reported_sql}) IS NOT NULL OR (({input_sql}) IS NOT NULL AND ({output_sql}) IS NOT NULL))"
    );
    let known_sql = format!(
        "(({reported_sql}) IS NOT NULL OR ({input_sql}) IS NOT NULL OR ({output_sql}) IS NOT NULL)"
    );
    let effective_sql = format!(
        "CASE WHEN ({reported_sql}) IS NOT NULL THEN ({reported_sql}) WHEN ({input_sql}) IS NOT NULL OR ({output_sql}) IS NOT NULL THEN COALESCE(({input_sql}), 0) + COALESCE(({output_sql}), 0) ELSE NULL END"
    );
    let sql = format!(
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
            COALESCE(SUM({reported_sql}), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' THEN {effective_sql} ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND {complete_sql} THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND NOT {complete_sql} THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND ({reported_sql}) IS NULL
                      AND {complete_sql} THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' AND NOT {complete_sql}
                      AND {known_sql} THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(m.estimated_cost_nanodollars), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant'
                      AND {complete_sql}
                      AND {cost_status_sql} = 'estimated' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant'
                      AND {complete_sql}
                      AND {cost_status_sql} = 'free' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant'
                      AND {complete_sql}
                      AND {cost_status_sql} = 'included' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant'
                      AND {complete_sql}
                      AND {cost_status_sql} = 'unknown' THEN 1 ELSE 0 END), 0)
        FROM messages m
        {where_clause}
        "#,
        cost_status_sql = cost_status_sql(),
        effective_sql = effective_sql,
        complete_sql = complete_sql,
        known_sql = known_sql,
        reported_sql = reported_sql,
    );
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
    if let Some(since_ms) = since_ms {
        query = query.bind(since_ms);
    }
    let row = query.fetch_one(&mut **conn).await?;
    let cache_read_tokens = row_u64(&row, 7)?;
    let billable_input_tokens = row_u64(&row, 4)?;
    let effective_total_tokens = row_u64(&row, 10)?;
    let accounted_provider_call_count = row_u64(&row, 11)?;
    let unaccounted_provider_call_count = row_u64(&row, 12)?;
    let derived_provider_call_count = row_u64(&row, 13)?;
    let partial_provider_call_count = row_u64(&row, 14)?;
    let estimated_pricing_count = row_u64(&row, 16)?;
    let free_pricing_count = row_u64(&row, 17)?;
    let included_pricing_count = row_u64(&row, 18)?;
    let unknown_pricing_count = row_u64(&row, 19)?;
    Ok(UsageWindowSummary {
        id: id.to_string(),
        label: label.to_string(),
        since_ms,
        session_count: row_u64(&row, 0)?,
        message_count: row_u64(&row, 1)?,
        assistant_message_count: row_u64(&row, 2)?,
        context_input_tokens: row_u64(&row, 3)?,
        billable_input_tokens,
        billable_output_tokens: row_u64(&row, 5)?,
        reasoning_tokens: row_u64(&row, 6)?,
        cache_read_tokens,
        cache_write_tokens: row_u64(&row, 8)?,
        effective_total_tokens,
        reported_total_tokens: row_u64(&row, 9)?,
        total_status: aggregate_usage_total_status(
            accounted_provider_call_count,
            unaccounted_provider_call_count,
            derived_provider_call_count,
            partial_provider_call_count,
        )
        .to_string(),
        accounted_provider_call_count,
        unaccounted_provider_call_count,
        estimated_cost_nanodollars: row.try_get(15)?,
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
        cache_read_percent: cache_read_percent(cache_read_tokens, row_u64(&row, 3)?),
    })
}

async fn usage_activity(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    generated_at_ms: i64,
    activity_days: usize,
) -> Result<UsageActivity> {
    let start_modifier = format!("-{} day", activity_days.saturating_sub(1));
    let reported_sql = usage_reported_total_sql();
    let input_sql = usage_context_input_sql();
    let output_sql = usage_output_sql();
    let complete_sql = format!(
        "(({reported_sql}) IS NOT NULL OR (({input_sql}) IS NOT NULL AND ({output_sql}) IS NOT NULL))"
    );
    let known_sql = format!(
        "(({reported_sql}) IS NOT NULL OR ({input_sql}) IS NOT NULL OR ({output_sql}) IS NOT NULL)"
    );
    let effective_sql = format!(
        "CASE WHEN ({reported_sql}) IS NOT NULL THEN ({reported_sql}) WHEN ({input_sql}) IS NOT NULL OR ({output_sql}) IS NOT NULL THEN COALESCE(({input_sql}), 0) + COALESCE(({output_sql}), 0) ELSE NULL END"
    );
    let sql = format!(
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
                COALESCE(SUM({reported_sql}), 0) AS reported_total_tokens,
                COALESCE(SUM(CASE WHEN m.role = 'assistant' THEN {effective_sql} ELSE 0 END), 0)
                    AS effective_total_tokens,
                COALESCE(SUM(CASE WHEN m.role = 'assistant' AND {complete_sql} THEN 1 ELSE 0 END), 0)
                    AS accounted_provider_call_count,
                COALESCE(SUM(CASE WHEN m.role = 'assistant' AND NOT {complete_sql} THEN 1 ELSE 0 END), 0)
                    AS unaccounted_provider_call_count,
                COALESCE(SUM(CASE WHEN m.role = 'assistant' AND ({reported_sql}) IS NULL
                          AND {complete_sql} THEN 1 ELSE 0 END), 0)
                    AS derived_provider_call_count,
                COALESCE(SUM(CASE WHEN m.role = 'assistant' AND NOT {complete_sql}
                          AND {known_sql} THEN 1 ELSE 0 END), 0)
                    AS partial_provider_call_count,
                COALESCE(SUM(m.context_input_tokens), 0) AS context_input_tokens,
                COALESCE(SUM(m.cache_read_tokens), 0) AS cache_read_tokens,
                COALESCE(SUM(m.cache_write_tokens), 0) AS cache_write_tokens,
                COALESCE(SUM(m.estimated_cost_nanodollars), 0) AS estimated_cost_nanodollars,
                COALESCE(SUM(CASE WHEN m.role = 'assistant'
                          AND {complete_sql}
                          AND {cost_status_sql} = 'estimated' THEN 1 ELSE 0 END), 0)
                    AS estimated_pricing_count,
                COALESCE(SUM(CASE WHEN m.role = 'assistant'
                          AND {complete_sql}
                          AND {cost_status_sql} = 'free' THEN 1 ELSE 0 END), 0)
                    AS free_pricing_count,
                COALESCE(SUM(CASE WHEN m.role = 'assistant'
                          AND {complete_sql}
                          AND {cost_status_sql} = 'included' THEN 1 ELSE 0 END), 0)
                    AS included_pricing_count,
                COALESCE(SUM(CASE WHEN m.role = 'assistant'
                          AND {complete_sql}
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
            COALESCE(daily.effective_total_tokens, 0),
            COALESCE(daily.accounted_provider_call_count, 0),
            COALESCE(daily.unaccounted_provider_call_count, 0),
            COALESCE(daily.derived_provider_call_count, 0),
            COALESCE(daily.partial_provider_call_count, 0),
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
        effective_sql = effective_sql,
        complete_sql = complete_sql,
        known_sql = known_sql,
        reported_sql = reported_sql,
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(generated_at_ms)
        .bind(start_modifier)
        .fetch_all(&mut **conn)
        .await?;
    let mut days = Vec::new();
    for row in rows {
        let accounted_provider_call_count = row_u64(&row, 5)?;
        let unaccounted_provider_call_count = row_u64(&row, 6)?;
        let derived_provider_call_count = row_u64(&row, 7)?;
        let partial_provider_call_count = row_u64(&row, 8)?;
        let estimated_pricing_count = row_u64(&row, 13)?;
        let free_pricing_count = row_u64(&row, 14)?;
        let included_pricing_count = row_u64(&row, 15)?;
        let unknown_pricing_count = row_u64(&row, 16)?;
        days.push(UsageActivityDay {
            date: row.try_get(0)?,
            session_count: row_u64(&row, 1)?,
            message_count: row_u64(&row, 2)?,
            effective_total_tokens: row_u64(&row, 4)?,
            reported_total_tokens: row_u64(&row, 3)?,
            total_status: aggregate_usage_total_status(
                accounted_provider_call_count,
                unaccounted_provider_call_count,
                derived_provider_call_count,
                partial_provider_call_count,
            )
            .to_string(),
            accounted_provider_call_count,
            unaccounted_provider_call_count,
            context_input_tokens: row_u64(&row, 9)?,
            cache_read_tokens: row_u64(&row, 10)?,
            cache_write_tokens: row_u64(&row, 11)?,
            estimated_cost_nanodollars: row.try_get(12)?,
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
        });
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

fn row_u64(row: &sqlx::sqlite::SqliteRow, index: usize) -> Result<u64> {
    row.try_get::<i64, _>(index)
        .map(|value| value.max(0) as u64)
        .map_err(Into::into)
}

pub(crate) struct StatsScope {
    pub(crate) cwd: Option<String>,
    pub(crate) cutoff_ms: Option<i64>,
    pub(crate) limit: usize,
}

pub(crate) async fn totals(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    scope: &StatsScope,
) -> Result<Value> {
    let sql = format!(
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
    );
    let row = bind_scope_query(sqlx::query(sqlx::AssertSqlSafe(sql)), scope)
        .fetch_one(&mut **conn)
        .await?;
    Ok(json!({
        "sessions": row.try_get::<i64, _>(0)?,
        "messages": row.try_get::<i64, _>(1)?,
        "context_input_tokens": row.try_get::<i64, _>(2)?,
        "billable_input_tokens": row.try_get::<i64, _>(3)?,
        "billable_output_tokens": row.try_get::<i64, _>(4)?,
        "reasoning_tokens": row.try_get::<i64, _>(5)?,
        "cache_read_tokens": row.try_get::<i64, _>(6)?,
        "cache_write_tokens": row.try_get::<i64, _>(7)?,
        "reported_total_tokens": row.try_get::<i64, _>(8)?,
        "estimated_cost_nanodollars": row.try_get::<i64, _>(9)?,
        "estimated_priced_messages": row.try_get::<i64, _>(10)?,
        "free_priced_messages": row.try_get::<i64, _>(11)?,
        "included_priced_messages": row.try_get::<i64, _>(12)?,
        "unknown_priced_messages": row.try_get::<i64, _>(13)?,
    }))
}

pub(crate) async fn provider_models(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    scope: &StatsScope,
) -> Result<Value> {
    let sql = format!(
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
        scope_parameter_count(scope) + 1
    );
    let rows = bind_scope_query(sqlx::query(sqlx::AssertSqlSafe(sql)), scope)
        .bind(scope.limit as i64)
        .fetch_all(&mut **conn)
        .await?;
    Ok(Value::Array(
        rows.into_iter()
            .map(|row| {
                Ok(json!({
                    "provider": row.try_get::<String, _>(0)?,
                    "model": row.try_get::<String, _>(1)?,
                    "messages": row.try_get::<i64, _>(2)?,
                    "reported_total_tokens": row.try_get::<i64, _>(3)?,
                    "estimated_cost_nanodollars": row.try_get::<i64, _>(4)?,
                }))
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}

pub(crate) async fn top_tools(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    scope: &StatsScope,
) -> Result<Value> {
    let sql = format!(
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
        scope_parameter_count(scope) + 1
    );
    let rows = bind_scope_query(sqlx::query(sqlx::AssertSqlSafe(sql)), scope)
        .bind(scope.limit as i64)
        .fetch_all(&mut **conn)
        .await?;
    Ok(Value::Array(
        rows.into_iter()
            .map(|row| {
                Ok(json!({
                    "tool": row.try_get::<String, _>(0)?,
                    "calls": row.try_get::<i64, _>(1)?,
                }))
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}

pub(crate) async fn top_sessions(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    scope: &StatsScope,
) -> Result<Value> {
    let sql = format!(
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
        scope_parameter_count(scope) + 1
    );
    let rows = bind_scope_query(sqlx::query(sqlx::AssertSqlSafe(sql)), scope)
        .bind(scope.limit as i64)
        .fetch_all(&mut **conn)
        .await?;
    Ok(Value::Array(
        rows.into_iter()
            .map(|row| {
                Ok(json!({
                    "session": row.try_get::<String, _>(0)?,
                    "title": row.try_get::<Option<String>, _>(1)?,
                    "cwd": row.try_get::<String, _>(2)?,
                    "provider": row.try_get::<String, _>(3)?,
                    "model": row.try_get::<String, _>(4)?,
                    "reported_total_tokens": row.try_get::<i64, _>(5)?,
                    "estimated_cost_nanodollars": row.try_get::<i64, _>(6)?,
                    "updated_at_ms": row.try_get::<i64, _>(7)?,
                }))
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}

pub(crate) fn scope_where_clause(scope: &StatsScope) -> &'static str {
    match (scope.cwd.is_some(), scope.cutoff_ms.is_some()) {
        (false, false) => "WHERE 1 = 1",
        (true, false) => "WHERE s.cwd = ?1",
        (false, true) => "WHERE s.updated_at_ms >= ?1",
        (true, true) => "WHERE s.cwd = ?1 AND s.updated_at_ms >= ?2",
    }
}

fn scope_parameter_count(scope: &StatsScope) -> usize {
    usize::from(scope.cwd.is_some()) + usize::from(scope.cutoff_ms.is_some())
}

fn bind_scope_query<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    scope: &'q StatsScope,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    if let Some(cwd) = &scope.cwd {
        query = query.bind(cwd);
    }
    if let Some(cutoff_ms) = scope.cutoff_ms {
        query = query.bind(cutoff_ms);
    }
    query
}
