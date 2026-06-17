#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn sqlite_schema_v15_stores_reasoning_only_in_message_json_and_metrics_separately() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("session");
    store
        .append_message_with_metrics(
            &session_id,
            &Message::Assistant {
                content: vec![
                    AssistantBlock::Reasoning {
                        text: "folded".to_string(),
                        provider_evidence: Some(json!({
                            "reasoning_details": [{ "type": "thinking", "text": "opaque" }]
                        })),
                    },
                    AssistantBlock::Text {
                        text: "visible".to_string(),
                    },
                ],
                timestamp_ms: 1,
                finish_reason: Some("stop".to_string()),
                outcome: Outcome::Normal,
                model: Some("model".to_string()),
                provider: Some("provider".to_string()),
            },
            Some(json!({"total_tokens": 12, "input_tokens": 5, "output_tokens": 7})),
            Some(json!({"provider_response_id": "resp_1", "model": "model"})),
        )
        .expect("append");

    let conn = Connection::open(&db).expect("db");
    let columns = conn
        .prepare("PRAGMA table_info(messages)")
        .expect("schema stmt")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("schema rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("columns");
    assert!(!columns.iter().any(|name| name == "reasoning_json"));
    assert!(!columns.iter().any(|name| name == "reasoning_content"));
    assert!(!columns.iter().any(|name| name == "reasoning_details_json"));

    let (message_json, usage_json, metadata_json): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT message_json, usage_json, metadata_json FROM messages WHERE session_id = ?1",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("message row");
    let message: Value = serde_json::from_str(&message_json).expect("message");
    assert_eq!(message["content"][0]["type"], "reasoning");
    assert_eq!(message["content"][0]["text"], "folded");
    assert_eq!(
        message["content"][0]["provider_evidence"]["reasoning_details"][0]["type"],
        "thinking"
    );
    assert!(message.get("reasoning_content").is_none());
    assert!(message.get("reasoning_details").is_none());
    assert!(message.get("usage").is_none());
    assert!(message.get("metadata").is_none());

    let usage: Value = serde_json::from_str(&usage_json.expect("usage")).expect("usage json");
    let metadata: Value =
        serde_json::from_str(&metadata_json.expect("metadata")).expect("metadata json");
    assert_eq!(usage["total_tokens"], 12);
    assert_eq!(metadata["provider_response_id"], "resp_1");

    let summaries = store
        .load_sanitized_message_summaries(&session_id)
        .expect("summaries");
    assert_eq!(summaries[0].usage.as_ref().unwrap()["total_tokens"], 12);
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["provider_response_id"],
        "resp_1"
    );
    let sanitized = serde_json::to_string(&summaries[0].message).expect("sanitized");
    assert!(!sanitized.contains("folded"));

    let tui_summaries = store
        .load_tui_message_summaries(&session_id)
        .expect("tui summaries");
    let tui_message = serde_json::to_value(&tui_summaries[0].message).expect("tui message");
    assert_eq!(tui_message["content"][0]["type"], "reasoning");
    assert_eq!(tui_message["content"][0]["text"], "folded");
    assert!(tui_message["content"][0].get("provider_evidence").is_none());
}

#[test]
pub(crate) fn session_compaction_checkpoint_respects_revert_boundary() {
    let temp = tempdir().expect("temp");
    let store = SqliteStore::open(&temp.path().join("state.db")).expect("store");
    let session = store
        .create_session_with_metadata(temp.path(), "run", "model", "provider", None)
        .expect("session");
    store
        .append_message(&session, &psychevo_agent_core::user_text_message("one"))
        .expect("message one");
    store
        .append_message(&session, &psychevo_agent_core::user_text_message("two"))
        .expect("message two");
    store
        .append_message(&session, &psychevo_agent_core::user_text_message("three"))
        .expect("message three");

    let record = store
        .append_session_compaction(SessionCompactionInput {
            session_id: session.clone(),
            reason: "manual".to_string(),
            summary_text: "summary".to_string(),
            first_kept_session_seq: 3,
            created_after_session_seq: 3,
            tokens_before: Some(300),
            tokens_after: Some(120),
            summary_provider: "provider".to_string(),
            summary_model: "model".to_string(),
            instructions: None,
            metadata: None,
        })
        .expect("compaction");
    assert_eq!(
        store
            .latest_valid_session_compaction(&session)
            .expect("latest")
            .map(|record| record.id),
        Some(record.id)
    );

    store
        .set_session_revert_state(
            &session,
            crate::store::SessionRevertState {
                start_seq: 3,
                original_snapshot: "snapshot".to_string(),
            },
        )
        .expect("revert");
    assert_eq!(
        store
            .latest_valid_session_compaction(&session)
            .expect("latest after revert"),
        None
    );
}

#[test]
pub(crate) fn sqlite_stats_aggregate_accounting_columns() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "run", "mimo-v2.5-pro", "xiaomi", None)
        .expect("session");
    store
        .append_message_with_metrics_and_accounting(
            &session_id,
            &Message::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "done".to_string(),
                }],
                timestamp_ms: 1,
                finish_reason: Some("stop".to_string()),
                outcome: Outcome::Normal,
                model: Some("mimo-v2.5-pro".to_string()),
                provider: Some("xiaomi".to_string()),
            },
            Some(json!({
                "input_tokens": 120,
                "output_tokens": 30,
                "total_tokens": 150
            })),
            None,
            Some(MessageAccounting {
                context_input_tokens: Some(120),
                billable_input_tokens: Some(100),
                billable_output_tokens: Some(25),
                reasoning_tokens: Some(5),
                cache_read_tokens: Some(10),
                cache_write_tokens: Some(10),
                reported_total_tokens: Some(150),
                estimated_cost_nanodollars: Some(42),
                pricing_source: Some("test".to_string()),
                pricing_tier: Some("standard".to_string()),
                cost_status: Some(crate::types::CostStatus::Estimated),
                pricing_missing_reason: None,
                pricing_version: None,
            }),
        )
        .expect("append");

    let report = usage_stats(StatsOptions {
        state: StateRuntime::open(&db).expect("state runtime"),
        workdir,
        all: false,
        days: None,
        limit: 5,
    })
    .expect("stats");
    assert_eq!(report["totals"]["estimated_cost_nanodollars"], 42);
    assert_eq!(report["totals"]["cache_write_tokens"], 10);
    assert_eq!(report["provider_models"][0]["model"], "mimo-v2.5-pro");
}

#[test]
pub(crate) fn session_usage_summary_sums_accounting_and_handles_missing_accounting() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "run", "mimo-v2.5-pro", "xiaomi", None)
        .expect("session");
    store
        .append_message_with_metrics_and_accounting(
            &session_id,
            &Message::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "done".to_string(),
                }],
                timestamp_ms: 1,
                finish_reason: Some("stop".to_string()),
                outcome: Outcome::Normal,
                model: Some("mimo-v2.5-pro".to_string()),
                provider: Some("xiaomi".to_string()),
            },
            Some(json!({
                "input_tokens": 120,
                "output_tokens": 30,
                "total_tokens": 150
            })),
            None,
            Some(MessageAccounting {
                context_input_tokens: Some(120),
                billable_input_tokens: Some(100),
                billable_output_tokens: Some(25),
                reasoning_tokens: Some(5),
                cache_read_tokens: Some(10),
                cache_write_tokens: Some(10),
                reported_total_tokens: Some(150),
                estimated_cost_nanodollars: Some(42),
                pricing_source: Some("test".to_string()),
                pricing_tier: None,
                cost_status: Some(crate::types::CostStatus::Estimated),
                pricing_missing_reason: None,
                pricing_version: None,
            }),
        )
        .expect("append accounting");
    store
        .append_message_with_metrics(
            &session_id,
            &Message::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "usage only".to_string(),
                }],
                timestamp_ms: 2,
                finish_reason: Some("stop".to_string()),
                outcome: Outcome::Normal,
                model: Some("mimo-v2.5-pro".to_string()),
                provider: Some("xiaomi".to_string()),
            },
            Some(json!({
                "input_tokens": 50,
                "output_tokens": 10,
                "total_tokens": 60,
                "reasoning_tokens": 2,
                "cached_tokens": 25
            })),
            None,
        )
        .expect("append usage");
    store
        .append_message(
            &session_id,
            &Message::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "no usage".to_string(),
                }],
                timestamp_ms: 3,
                finish_reason: Some("stop".to_string()),
                outcome: Outcome::Normal,
                model: Some("mimo-v2.5-pro".to_string()),
                provider: Some("xiaomi".to_string()),
            },
        )
        .expect("append missing");

    let summary = session_usage_summary(SessionUsageOptions {
        state: StateRuntime::open(&db).expect("state runtime"),
        session_id: session_id.clone(),
    })
    .expect("summary");
    assert_eq!(summary.session_id, session_id);
    assert_eq!(summary.message_count, 3);
    assert_eq!(summary.assistant_message_count, 3);
    assert_eq!(summary.context_input_tokens, 170);
    assert_eq!(summary.billable_input_tokens, 125);
    assert_eq!(summary.billable_output_tokens, 33);
    assert_eq!(summary.reasoning_tokens, 7);
    assert_eq!(summary.cache_read_tokens, 35);
    assert_eq!(summary.cache_write_tokens, 10);
    assert_eq!(summary.reported_total_tokens, 210);
    assert_eq!(summary.estimated_cost_nanodollars, 42);
    assert_eq!(summary.unknown_pricing_count, 1);
    assert_eq!(
        summary
            .cache_read_percent
            .map(|value| (value * 10.0).round() / 10.0),
        Some(21.9)
    );
}

#[test]
pub(crate) fn session_usage_summary_respects_session_and_revert_boundaries() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let first = store
        .create_session_with_metadata(&workdir, "run", "model-a", "provider-a", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&workdir, "run", "model-b", "provider-b", None)
        .expect("second");
    for (timestamp_ms, session_id, tokens) in [
        (1_i64, &first, 100_u64),
        (2_i64, &first, 200_u64),
        (3_i64, &second, 900_u64),
    ] {
        store
            .append_message_with_metrics_and_accounting(
                session_id,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "answer".to_string(),
                    }],
                    timestamp_ms,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
                None,
                None,
                Some(MessageAccounting {
                    context_input_tokens: Some(tokens),
                    billable_input_tokens: Some(tokens),
                    billable_output_tokens: None,
                    reasoning_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    reported_total_tokens: Some(tokens),
                    estimated_cost_nanodollars: None,
                    pricing_source: Some("test".to_string()),
                    pricing_tier: None,
                    cost_status: Some(crate::types::CostStatus::Unknown),
                    pricing_missing_reason: Some("missing_output_price".to_string()),
                    pricing_version: None,
                }),
            )
            .expect("append");
    }
    store
        .set_session_revert_state(
            &first,
            crate::store::SessionRevertState {
                start_seq: 2,
                original_snapshot: "snapshot".to_string(),
            },
        )
        .expect("revert");

    let first_summary = session_usage_summary(SessionUsageOptions {
        state: StateRuntime::open(&db).expect("state runtime"),
        session_id: first,
    })
    .expect("first summary");
    let second_summary = session_usage_summary(SessionUsageOptions {
        state: StateRuntime::open(&db).expect("state runtime"),
        session_id: second,
    })
    .expect("second summary");
    assert_eq!(first_summary.context_input_tokens, 100);
    assert_eq!(first_summary.reported_total_tokens, 100);
    assert_eq!(second_summary.context_input_tokens, 900);
    assert_eq!(second_summary.reported_total_tokens, 900);
}

#[test]
pub(crate) fn usage_read_returns_all_recent_windows_and_activity() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("session");
    let now = psychevo_agent_core::now_ms();
    for (timestamp_ms, total, cache_read, billable_input) in [
        (now, 100_u64, 20_u64, 80_u64),
        (
            now.saturating_sub(40 * 86_400_000),
            200_u64,
            40_u64,
            160_u64,
        ),
    ] {
        store
            .append_message_with_metrics_and_accounting(
                &session_id,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "answer".to_string(),
                    }],
                    timestamp_ms,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
                None,
                None,
                Some(MessageAccounting {
                    context_input_tokens: Some(total),
                    billable_input_tokens: Some(billable_input),
                    billable_output_tokens: Some(10),
                    reasoning_tokens: None,
                    cache_read_tokens: Some(cache_read),
                    cache_write_tokens: None,
                    reported_total_tokens: Some(total),
                    estimated_cost_nanodollars: Some(total as i64),
                    pricing_source: Some("test".to_string()),
                    pricing_tier: Some("standard".to_string()),
                    cost_status: Some(crate::types::CostStatus::Estimated),
                    pricing_missing_reason: None,
                    pricing_version: None,
                }),
            )
            .expect("append");
    }

    let result = usage_read(UsageReadOptions {
        state: StateRuntime::open(&db).expect("state runtime"),
        activity_days: 365,
    })
    .expect("usage read");
    let all = result
        .windows
        .iter()
        .find(|window| window.id == "all")
        .unwrap();
    let last_30 = result
        .windows
        .iter()
        .find(|window| window.id == "30d")
        .unwrap();
    let last_7 = result
        .windows
        .iter()
        .find(|window| window.id == "7d")
        .unwrap();
    assert_eq!(all.reported_total_tokens, 300);
    assert_eq!(last_30.reported_total_tokens, 100);
    assert_eq!(last_7.reported_total_tokens, 100);
    assert_eq!(last_7.cache_read_percent, Some(20.0));
    assert_eq!(result.activity.days.len(), 365);
    assert!(
        result
            .activity
            .days
            .iter()
            .any(|day| day.reported_total_tokens == 100)
    );
}

#[test]
pub(crate) fn accounting_uses_cache_reasoning_and_over_200k_pricing() {
    let metadata = ModelMetadata {
        cost: Some(ModelCost {
            input: Some(1.0),
            output: Some(2.0),
            cache_read: Some(0.1),
            cache_write: Some(0.2),
            request: None,
            context_over_200k: Some(ModelCostTier {
                input: Some(3.0),
                output: Some(4.0),
                cache_read: Some(0.3),
                cache_write: Some(0.4),
            }),
            source: Some("test-pricing".to_string()),
            version: None,
        }),
        ..Default::default()
    };
    let accounting = crate::accounting::account_usage(
        Some(&json!({
            "input_tokens": 250_020,
            "output_tokens": 30,
            "total_tokens": 250_050,
            "reasoning_tokens": 5,
            "cached_tokens": 10,
            "cache_write_tokens": 10
        })),
        &metadata,
    )
    .expect("accounting");
    assert_eq!(accounting.billable_input_tokens, Some(250_000));
    assert_eq!(accounting.billable_output_tokens, Some(25));
    assert_eq!(
        accounting.pricing_tier.as_deref(),
        Some("context_over_200k")
    );
    assert_eq!(accounting.pricing_source.as_deref(), Some("test-pricing"));
    assert_eq!(
        accounting.estimated_cost_nanodollars,
        Some(250_000 * 3_000 + 25 * 4_000 + 5 * 4_000 + 10 * 300 + 10 * 400)
    );
}

#[test]
pub(crate) fn accounting_marks_missing_cache_pricing_unknown() {
    let metadata = ModelMetadata {
        cost: Some(ModelCost {
            input: Some(1.0),
            output: Some(2.0),
            cache_read: None,
            cache_write: None,
            request: None,
            context_over_200k: None,
            source: Some("test-pricing".to_string()),
            version: None,
        }),
        ..Default::default()
    };
    let accounting = crate::accounting::account_usage(
        Some(&json!({
            "input_tokens": 100,
            "output_tokens": 10,
            "total_tokens": 110,
            "cached_tokens": 25
        })),
        &metadata,
    )
    .expect("accounting");
    assert_eq!(
        accounting.cost_status,
        Some(crate::types::CostStatus::Unknown)
    );
    assert_eq!(accounting.estimated_cost_nanodollars, None);
    assert_eq!(
        accounting.pricing_missing_reason.as_deref(),
        Some("missing_cache_read_price")
    );
}

pub(crate) fn sqlite_columns(conn: &Connection, table: &str) -> Vec<String> {
    conn.prepare(&format!("PRAGMA table_info({table})"))
        .expect("schema stmt")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("schema rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("columns")
}
