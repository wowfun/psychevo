#[cfg(test)]
mod tests {
    use super::*;
    use psychevo_ai::Outcome;

    #[test]
    fn trace_path_is_disabled_for_memory_db() {
        assert_eq!(
            session_trace_path(Path::new(":memory:"), "session-1").expect("path"),
            None
        );
    }

    #[test]
    fn trace_path_rejects_unsafe_session_id() {
        assert!(session_trace_path(Path::new("/tmp/state.db"), "../bad").is_err());
    }

    #[test]
    fn read_session_trace_ignores_malformed_final_line() {
        let temp = tempfile::tempdir().expect("temp");
        let db = temp.path().join("state.db");
        let trace = temp.path().join("sessions").join("s1");
        fs::create_dir_all(&trace).expect("trace dir");
        fs::write(
            trace.join("events.jsonl"),
            concat!(
                "{\"schema_version\":1,\"seq\":1,\"kind\":\"agent_start\"}\n",
                "{\"schema_version\":1,\"seq\":"
            ),
        )
        .expect("trace");

        let result = read_session_trace(
            &db,
            "s1",
            SessionTraceReadOptions {
                after_seq: None,
                limit: Some(10),
            },
        );
        assert!(result.available);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0]["seq"], 1);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn append_trace_record_writes_redacted_bounded_event() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("sessions").join("s1").join("events.jsonl");
        append_trace_record(
            &path,
            "s1",
            "invocation-1",
            1,
            SessionTraceDraft {
                kind: "tool_execution_start".to_string(),
                timestamp_ms: 10,
                monotonic_offset_ms: 0,
                turn_index: Some(0),
                correlation: json!({"tool_call_id": "call-1"}),
                payload: bounded_redacted_value(&json!({
                    "args": {
                        "api_key": "secret",
                        "path": "README.md"
                    }
                })),
            },
        )
        .expect("append");

        let result = read_session_trace(
            &temp.path().join("state.db"),
            "s1",
            SessionTraceReadOptions::default(),
        );
        assert_eq!(result.events.len(), 1);
        let event = &result.events[0];
        assert_eq!(event["schema_version"], SESSION_TRACE_SCHEMA_VERSION);
        assert_eq!(event["seq"], 1);
        assert_eq!(event["payload"]["args"]["api_key"], "<redacted>");
        assert_eq!(event["payload"]["args"]["path"], "README.md");
    }

    #[test]
    fn compact_trace_coalesces_high_frequency_events_into_run_summary() {
        let mut stats = SessionTraceStats::default();
        let high_frequency_events = [
            AgentEvent::MessageUpdate {
                message: assistant_message("partial"),
            },
            AgentEvent::ReasoningDelta {
                text: "thinking".to_string(),
            },
            AgentEvent::ToolCallPending {
                tool_call_id: "call-1".to_string(),
                tool_name: "read".to_string(),
                arguments_json: "{\"path\":\"README.md\"}".to_string(),
                content_index: 0,
                call_index: 0,
                display: None,
            },
            AgentEvent::ToolExecutionUpdate {
                tool_call_id: "call-1".to_string(),
                tool_name: "read".to_string(),
                partial_result: json!({
                    "status": "streaming",
                    "output": "z".repeat(10_000),
                }),
            },
        ];
        for event in &high_frequency_events {
            assert!(trace_drafts_from_agent_event(event, None, 0, Some(0), &mut stats).is_empty());
        }

        let drafts = trace_drafts_from_agent_event(
            &AgentEvent::AgentEnd {
                outcome: Outcome::Normal,
                messages: Vec::new(),
                terminal_reason: None,
            },
            None,
            10,
            Some(0),
            &mut stats,
        );

        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].kind, "agent_end");
        assert_eq!(drafts[1].kind, "run_summary");
        let summary = &drafts[1].payload;
        assert_eq!(summary["summary_kind"], "accounting_footer");
        assert_eq!(summary["coalesced_counts"]["message_update"], 1);
        assert_eq!(summary["coalesced_counts"]["reasoning_delta"], 1);
        assert_eq!(summary["coalesced_counts"]["tool_call_pending"], 1);
        assert_eq!(summary["coalesced_counts"]["tool_execution_update"], 1);
        assert_eq!(summary["turns"][0]["coalesced_events"], 4);
        assert_eq!(summary["turns"][0]["turn_index"], 0);
        assert_eq!(
            summary["coalesced_by_tool_name"]["read"]["tool_call_pending"],
            1
        );
        assert_eq!(
            summary["coalesced_by_tool_name"]["read"]["tool_execution_update"],
            1
        );
        assert!(summary.get("event_counts").is_none());
        assert!(summary.get("persisted_counts").is_none());
        assert!(summary.get("coalesced_tool_events").is_none());
        assert!(summary.get("tools").is_none());
        assert!(summary.get("generations").is_none());
        let encoded = serde_json::to_string(summary).expect("summary json");
        assert!(!encoded.contains("call-1"));
        assert!(!encoded.contains(&"z".repeat(128)));
    }

    #[test]
    fn compact_trace_keeps_minimal_message_end_summary_without_full_message() {
        let mut stats = SessionTraceStats::default();
        let accounting = MessageAccounting {
            context_input_tokens: Some(1),
            billable_input_tokens: Some(2),
            billable_output_tokens: Some(3),
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reported_total_tokens: None,
            estimated_cost_nanodollars: None,
            pricing_source: Some("test".to_string()),
            pricing_tier: Some("standard".to_string()),
            cost_status: Some(crate::types::CostStatus::Unknown),
            pricing_missing_reason: Some("missing_output_price".to_string()),
            pricing_version: None,
        };
        let drafts = trace_drafts_from_agent_event(
            &AgentEvent::MessageEnd {
                message: assistant_message("final answer"),
                usage: Some(json!({"input_tokens": 3, "output_tokens": 4})),
                metadata: Some(json!({
                    "elapsed_ms": 123,
                    "debug_body": "x".repeat(10_000),
                })),
            },
            Some(&accounting),
            12,
            Some(0),
            &mut stats,
        );

        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].kind, "message_end");
        let payload = &drafts[0].payload;
        assert_eq!(payload["role"], "assistant");
        assert_eq!(payload["summary"]["text_chars"], 12);
        assert_eq!(payload["summary"]["finish_reason"], "stop");
        assert_eq!(payload["summary"]["outcome"], "normal");
        assert!(payload["summary"].get("model").is_none());
        assert!(payload["summary"].get("provider").is_none());
        assert!(payload.get("usage").is_none());
        assert!(payload.get("metadata").is_none());
        assert!(payload.get("accounting").is_none());
        assert!(payload.get("message").is_none());
    }

    #[test]
    fn compact_trace_summarizes_tool_payloads_without_body_preview_or_display() {
        let mut stats = SessionTraceStats::default();
        let long_body = "c".repeat(10_000);
        let long_cmd = "run ".to_string() + &"x".repeat(220);
        let start = trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionStart {
                tool_call_id: "call-1".to_string(),
                tool_name: "write".to_string(),
                args: json!({
                    "cmd": long_cmd,
                    "path": "README.md",
                    "content": long_body,
                    "api_key": "secret",
                }),
                started_at_ms: 20,
                display: Some(psychevo_agent_core::ToolDisplaySpec::for_name("write")),
            },
            None,
            20,
            Some(0),
            &mut stats,
        );

        assert_eq!(start.len(), 1);
        let payload = &start[0].payload;
        assert!(payload.get("args").is_none());
        assert_eq!(payload["args_summary"]["type"], "object");
        assert_eq!(payload["args_summary"]["field_count"], 4);
        assert_eq!(payload["args_summary"]["title"]["path"], "README.md");
        assert_eq!(payload["args_summary"]["title"]["cmd"]["truncated"], true);
        assert_eq!(payload["args_summary"]["title"]["cmd"]["chars"], 224);
        assert_eq!(
            payload["args_summary"]["title"]["cmd"]["prefix"]
                .as_str()
                .expect("cmd prefix")
                .chars()
                .count(),
            TRACE_TITLE_STRING_CHARS
        );
        assert!(payload["args_summary"].get("fields").is_none());
        assert!(payload.get("display").is_none());
        let encoded = serde_json::to_string(payload).expect("payload json");
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains(&"c".repeat(128)));

        let result = trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionEnd {
                tool_call_id: "call-1".to_string(),
                tool_name: "write".to_string(),
                result: json!({
                    "status": "ok",
                    "output": "y".repeat(10_000),
                }),
                outcome: Outcome::Normal,
                elapsed_ms: 34,
                display: Some(psychevo_agent_core::ToolDisplaySpec::for_name("write")),
            },
            None,
            54,
            Some(0),
            &mut stats,
        );

        assert_eq!(result.len(), 1);
        let payload = &result[0].payload;
        assert!(payload.get("result").is_none());
        assert_eq!(payload["result_summary"]["type"], "object");
        assert_eq!(payload["result_summary"]["title"]["status"], "ok");
        assert!(payload["result_summary"].get("fields").is_none());
        assert!(payload.get("display").is_none());
        let encoded = serde_json::to_string(payload).expect("payload json");
        assert!(!encoded.contains(&"y".repeat(128)));
    }

    #[test]
    fn compact_trace_keeps_event_types_with_trimmed_payloads() {
        let mut stats = SessionTraceStats::default();
        let mut drafts = Vec::new();
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::TurnStart { turn_index: 7 },
            None,
            1,
            Some(7),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::GenerationStart {
                generation_id: "generation-1".to_string(),
                provider: "provider".to_string(),
                model: "model".to_string(),
                message_count: 2,
                tool_count: 1,
                started_at_ms: 10,
            },
            None,
            10,
            Some(7),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::GenerationEnd {
                generation_id: "generation-1".to_string(),
                provider: "provider".to_string(),
                model: "model".to_string(),
                outcome: Outcome::Normal,
                elapsed_ms: 30,
                usage: Some(json!({"input_tokens": 2})),
                metadata: Some(json!({})),
                error: None,
            },
            None,
            40,
            Some(7),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::ReasoningEnd {
                text: "hidden".to_string(),
            },
            None,
            41,
            Some(7),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::TurnEnd {
                turn_index: 7,
                outcome: Outcome::Normal,
            },
            None,
            42,
            Some(7),
            &mut stats,
        ));

        let kinds = drafts
            .iter()
            .map(|draft| draft.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                "turn_start",
                "generation_start",
                "generation_end",
                "reasoning_end",
                "turn_end"
            ]
        );
        assert_eq!(drafts[0].payload, json!({}));
        assert_eq!(drafts[1].payload["message_count"], 2);
        assert!(drafts[1].payload.get("provider").is_none());
        assert!(drafts[1].payload.get("model").is_none());
        assert_eq!(drafts[2].payload["elapsed_ms"], 30);
        assert_eq!(drafts[2].payload["usage"]["input_tokens"], 2);
        assert!(drafts[2].payload.get("metadata").is_none());
        assert_eq!(drafts[4].payload, json!({"outcome": "normal"}));
    }

    #[test]
    fn run_summary_does_not_duplicate_lifecycle_fact_details() {
        let mut stats = SessionTraceStats::default();
        let mut drafts = Vec::new();
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::GenerationStart {
                generation_id: "generation-1".to_string(),
                provider: "provider".to_string(),
                model: "model".to_string(),
                message_count: 2,
                tool_count: 1,
                started_at_ms: 10,
            },
            None,
            10,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::GenerationEnd {
                generation_id: "generation-1".to_string(),
                provider: "provider".to_string(),
                model: "model".to_string(),
                outcome: Outcome::Normal,
                elapsed_ms: 30,
                usage: None,
                metadata: None,
                error: None,
            },
            None,
            40,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionStart {
                tool_call_id: "call-1".to_string(),
                tool_name: "write".to_string(),
                args: json!({"path": "README.md", "content": "x".repeat(10_000)}),
                started_at_ms: 45,
                display: None,
            },
            None,
            45,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionEnd {
                tool_call_id: "call-1".to_string(),
                tool_name: "write".to_string(),
                result: json!({"status": "ok", "output": "y".repeat(10_000)}),
                outcome: Outcome::Normal,
                elapsed_ms: 25,
                display: None,
            },
            None,
            70,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::AgentEnd {
                outcome: Outcome::Normal,
                messages: Vec::new(),
                terminal_reason: None,
            },
            None,
            75,
            Some(0),
            &mut stats,
        ));

        let summary = drafts
            .iter()
            .find(|draft| draft.kind == "run_summary")
            .expect("run summary");
        assert_eq!(summary.payload["summary_kind"], "accounting_footer");
        assert!(summary.payload.get("event_counts").is_none());
        assert!(summary.payload.get("persisted_counts").is_none());
        assert!(summary.payload.get("coalesced_tool_events").is_none());
        assert_eq!(summary.payload["coalesced_by_tool_name"], json!({}));
        assert_eq!(summary.payload["omitted_counts"]["turns"], 0);
        assert!(summary.payload.get("tools").is_none());
        assert!(summary.payload.get("generations").is_none());
        let encoded = serde_json::to_string(&summary.payload).expect("summary json");
        assert!(!encoded.contains("args_summary"));
        assert!(!encoded.contains("result_summary"));
        assert!(!encoded.contains("elapsed_ms"));
        assert!(!encoded.contains("outcome"));
        assert!(!encoded.contains(&"x".repeat(128)));
        assert!(!encoded.contains(&"y".repeat(128)));
    }

    #[test]
    fn compact_trace_hackernews_like_pending_updates_stay_bounded() {
        let mut stats = SessionTraceStats::default();
        let mut drafts = Vec::new();
        for index in 0..2_500 {
            drafts.extend(trace_drafts_from_agent_event(
                &AgentEvent::ToolCallPending {
                    tool_call_id: "call-1".to_string(),
                    tool_name: "hackernews-daily".to_string(),
                    arguments_json: format!("{{\"query\":\"item-{index}\"}}"),
                    content_index: 0,
                    call_index: 0,
                    display: None,
                },
                None,
                index,
                Some(0),
                &mut stats,
            ));
        }
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionStart {
                tool_call_id: "call-1".to_string(),
                tool_name: "hackernews-daily".to_string(),
                args: json!({"query": "frontpage"}),
                started_at_ms: 2_600,
                display: None,
            },
            None,
            2_600,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::ToolExecutionEnd {
                tool_call_id: "call-1".to_string(),
                tool_name: "hackernews-daily".to_string(),
                result: json!({"status": "ok", "items": 500}),
                outcome: Outcome::Normal,
                elapsed_ms: 50,
                display: None,
            },
            None,
            2_650,
            Some(0),
            &mut stats,
        ));
        drafts.extend(trace_drafts_from_agent_event(
            &AgentEvent::AgentEnd {
                outcome: Outcome::Normal,
                messages: Vec::new(),
                terminal_reason: None,
            },
            None,
            2_700,
            Some(0),
            &mut stats,
        ));

        assert!(drafts.len() <= 100);
        assert!(!drafts.iter().any(|draft| draft.kind == "tool_call_pending"));
        let summary = drafts
            .iter()
            .find(|draft| draft.kind == "run_summary")
            .expect("run summary");
        assert_eq!(
            summary.payload["coalesced_counts"]["tool_call_pending"],
            2_500
        );
        assert_eq!(summary.payload["summary_kind"], "accounting_footer");
        assert_eq!(
            summary.payload["coalesced_by_tool_name"]["hackernews-daily"]["tool_call_pending"],
            2_500
        );
        assert_eq!(
            summary.payload["coalesced_by_tool_name"]["hackernews-daily"]["tool_execution_update"],
            0
        );
        assert!(summary.payload.get("event_counts").is_none());
        assert!(summary.payload.get("persisted_counts").is_none());
        assert!(summary.payload.get("coalesced_tool_events").is_none());
        assert!(summary.payload.get("tools").is_none());
        assert!(summary.payload.get("generations").is_none());
        let encoded = serde_json::to_string(&summary.payload).expect("summary json");
        assert!(!encoded.contains("call-1"));
    }

    fn assistant_message(text: &str) -> Message {
        Message::Assistant {
            content: vec![AssistantBlock::Text {
                text: text.to_string(),
            }],
            timestamp_ms: 1,
            finish_reason: Some("stop".to_string()),
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        }
    }
}
