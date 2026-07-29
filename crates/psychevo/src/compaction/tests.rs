#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;
    use crate::context_usage::{ContextScope, ContextTokenizer, ContextTotal};
    use psychevo_agent_core::{AssistantBlock, ToolCallBlock, now_ms};
    use futures::stream;
    use psychevo_ai::{
        AdapterCall, AdapterFuture, AdapterStream, DeploymentConfig, FinishReason,
        FinishReasonKind, GenerationOutcome, LanguageAdapter, LanguageAdapterEvent,
        LanguageRequest, Outcome, Provider,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn record(session_seq: i64, message: Message) -> SessionMessageRecord {
        SessionMessageRecord {
            session_seq,
            message,
        }
    }

    fn previous_compaction(first_kept_session_seq: i64) -> SessionCompactionRecord {
        SessionCompactionRecord {
            id: 1,
            session_id: "session".to_string(),
            created_at_ms: now_ms(),
            reason: "manual".to_string(),
            summary_text: "previous summary".to_string(),
            first_kept_session_seq,
            created_after_session_seq: first_kept_session_seq,
            tokens_before: Some(100),
            tokens_after: Some(50),
            summary_provider: "mock".to_string(),
            summary_model: "mock-model".to_string(),
            instructions: None,
            metadata: None,
        }
    }

    fn snapshot(tokens: u64, context_limit: Option<u64>) -> ContextSnapshot {
        ContextSnapshot {
            event_type: "context_snapshot".to_string(),
            scope: ContextScope::LastProviderRequest,
            status: "estimated".to_string(),
            basis: "latest_provider_request".to_string(),
            applies_to_session_seq: None,
            session_id: Some("session".to_string()),
            provider: "mock".to_string(),
            model: "model".to_string(),
            mode: Some("default".to_string()),
            context_limit,
            tokenizer: ContextTokenizer {
                encoding: "o200k_base".to_string(),
                source: "fallback".to_string(),
                fallback: true,
            },
            total: ContextTotal {
                tokens,
                estimated_tokens: tokens,
                estimated: true,
                source: "estimate".to_string(),
                percent: context_limit.map(|limit| tokens as f64 / limit as f64 * 100.0),
            },
            categories: BTreeMap::new(),
            advice: Vec::new(),
        }
    }

    #[derive(Debug, Clone)]
    struct SummaryAdapter {
        finish_reason: FinishReason,
    }

    impl LanguageAdapter for SummaryAdapter {
        fn stream(
            &self,
            _call: AdapterCall<LanguageRequest>,
        ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
            let finish_reason = self.finish_reason.clone();
            Box::pin(async move {
                Ok(Box::pin(stream::iter([
                    Ok(LanguageAdapterEvent::TextStart { content_index: 0 }),
                    Ok(LanguageAdapterEvent::TextDelta {
                        content_index: 0,
                        delta: "partial summary".to_string(),
                    }),
                    Ok(LanguageAdapterEvent::TextEnd { content_index: 0 }),
                    Ok(LanguageAdapterEvent::Finish {
                        finish_reason: Some(finish_reason),
                    }),
                ])) as AdapterStream<_>)
            })
        }
    }

    fn summary_model(kind: FinishReasonKind) -> psychevo_ai::LanguageModel {
        Provider::builder(
            DeploymentConfig::new("summary", "test", "test://summary")
                .with_default_language_protocol("test"),
        )
        .language_adapter(SummaryAdapter {
            finish_reason: FinishReason {
                kind,
                raw: Some("test_terminal".to_string()),
            },
        })
        .build()
        .expect("summary provider")
        .language_model("summary-model")
        .expect("summary model")
    }

    #[derive(Debug, Clone)]
    struct CountingSummaryAdapter {
        calls: Arc<AtomicUsize>,
        fail_on_call: Option<usize>,
    }

    impl LanguageAdapter for CountingSummaryAdapter {
        fn stream(
            &self,
            call: AdapterCall<LanguageRequest>,
        ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
            let call_index = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            let fail = self.fail_on_call == Some(call_index);
            let max_output_tokens = call.request.settings.max_output_tokens;
            Box::pin(async move {
                if fail {
                    return Err(psychevo_ai::ProviderError::protocol(
                        "synthetic rolling summary failure",
                    ));
                }
                Ok(Box::pin(stream::iter([
                    Ok(LanguageAdapterEvent::TextStart { content_index: 0 }),
                    Ok(LanguageAdapterEvent::TextDelta {
                        content_index: 0,
                        delta: format!(
                            "rolling summary with output budget {}",
                            max_output_tokens.unwrap_or_default()
                        ),
                    }),
                    Ok(LanguageAdapterEvent::TextEnd { content_index: 0 }),
                    Ok(LanguageAdapterEvent::Finish {
                        finish_reason: Some(FinishReason {
                            kind: FinishReasonKind::Stop,
                            raw: Some("test_terminal".to_string()),
                        }),
                    }),
                ])) as AdapterStream<_>)
            })
        }
    }

    fn counting_summary_model(
        calls: Arc<AtomicUsize>,
        fail_on_call: Option<usize>,
    ) -> psychevo_ai::LanguageModel {
        Provider::builder(
            DeploymentConfig::new("summary", "test", "test://summary")
                .with_default_language_protocol("test"),
        )
        .language_adapter(CountingSummaryAdapter {
            calls,
            fail_on_call,
        })
        .build()
        .expect("summary provider")
        .language_model("summary-model")
        .expect("summary model")
    }

    fn resolved_summary_provider() -> crate::config::ResolvedRunProvider {
        crate::config::ResolvedRunProvider {
            provider: "summary".to_string(),
            display_label: "Summary".to_string(),
            model: "summary-model".to_string(),
            base_url: "test://summary".to_string(),
            api_key_env: None,
            api_key: String::new(),
            inference_idle_timeout_secs: 0,
            reasoning_effort: None,
            context_limit: None,
            metadata: Default::default(),
        }
    }

    #[tokio::test]
    async fn summary_generation_rejects_non_stop_terminal_text() {
        let resolved = resolved_summary_provider();
        let summary = generate_summary(
            summary_model(FinishReasonKind::Stop),
            &resolved,
            None,
            &[],
            None,
        )
        .await
        .expect("stop summary");
        assert_eq!(summary, "partial summary");
        for kind in [FinishReasonKind::Length, FinishReasonKind::ContentFilter] {
            let error = generate_summary(summary_model(kind), &resolved, None, &[], None)
                .await
                .expect_err("non-stop summary must fail");
            assert!(
                error.to_string().contains("did not complete normally"),
                "{error}"
            );
        }
        assert!(
            validate_summary_completion(GenerationOutcome::Aborted, None).is_err(),
            "aborted summary must fail"
        );
    }

    #[tokio::test]
    async fn rolling_summary_chunks_under_budget_and_sets_bounded_output() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut resolved = resolved_summary_provider();
        resolved.context_limit = Some(8_192);
        resolved.metadata.limits.context = Some(8_192);
        let messages = (1..=4)
            .map(|seq| record(seq, user_text_message("x".repeat(12_000))))
            .collect::<Vec<_>>();

        let summary = generate_summary(
            counting_summary_model(Arc::clone(&calls), None),
            &resolved,
            None,
            &messages,
            None,
        )
        .await
        .expect("rolling summary");

        assert!(calls.load(Ordering::SeqCst) >= 2);
        assert!(summary.contains("output budget 2048"));
    }

    #[tokio::test]
    async fn rolling_summary_failure_returns_before_any_checkpoint_write() {
        let store = StateRuntime::open(std::path::Path::new(":memory:"))
            .await
            .expect("store");
        let session = store
            .create_session(std::path::Path::new("."))
            .await
            .expect("session");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut resolved = resolved_summary_provider();
        resolved.context_limit = Some(8_192);
        resolved.metadata.limits.context = Some(8_192);
        let messages = (1..=4)
            .map(|seq| record(seq, user_text_message("x".repeat(12_000))))
            .collect::<Vec<_>>();

        let error = generate_summary(
            counting_summary_model(Arc::clone(&calls), Some(2)),
            &resolved,
            None,
            &messages,
            None,
        )
        .await
        .expect_err("second chunk failure");

        assert!(error.to_string().contains("synthetic rolling summary failure"));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(
            store
                .list_valid_session_compactions(&session)
                .await
                .expect("compactions")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn fixed_compaction_input_and_tiny_output_fail_before_provider_call() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tiny = resolved_summary_provider();
        tiny.context_limit = Some(1_024);
        tiny.metadata.limits.context = Some(1_024);
        let tiny_error = generate_summary(
            counting_summary_model(Arc::clone(&calls), None),
            &tiny,
            None,
            &[],
            None,
        )
        .await
        .expect_err("tiny output budget");
        assert!(tiny_error.to_string().contains("at least 512"));

        let mut bounded = resolved_summary_provider();
        bounded.context_limit = Some(8_192);
        bounded.metadata.limits.context = Some(8_192);
        let fixed_error = generate_summary(
            counting_summary_model(Arc::clone(&calls), None),
            &bounded,
            None,
            &[],
            Some(&"manual focus ".repeat(4_000)),
        )
        .await
        .expect_err("oversized fixed input");
        assert!(fixed_error.to_string().contains("fixed prompt"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn summary_units_keep_tool_calls_and_results_indivisible_in_time_order() {
        let call = ToolCallBlock {
            id: "call-atomic".to_string(),
            name: "read".to_string(),
            arguments: json!({}),
            arguments_json: "{}".to_string(),
            arguments_error: None,
            content_index: 0,
            call_index: 0,
        };
        let messages = vec![
            record(
                4,
                Message::ToolResult {
                    tool_call_id: "call-atomic".to_string(),
                    tool_name: "read".to_string(),
                    content: "result".to_string(),
                    is_error: false,
                    timestamp_ms: now_ms(),
                },
            ),
            record(1, user_text_message("old task")),
            record(
                2,
                Message::Assistant {
                    content: vec![AssistantBlock::ToolCall(call)],
                    timestamp_ms: now_ms(),
                    finish_reason: None,
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            record(3, user_text_message("intervening context")),
            record(5, user_text_message("next task")),
        ];

        let units = atomic_summary_units(&messages);

        assert_eq!(
            units
                .iter()
                .map(|unit| unit.iter().map(|record| record.session_seq).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            vec![vec![1], vec![2, 3, 4], vec![5]]
        );
    }

    async fn auto_check_options(
        db_path: PathBuf,
        cwd: PathBuf,
        psychevo_home: PathBuf,
    ) -> AutoCompactionCheckOptions {
        AutoCompactionCheckOptions {
            state: StateRuntime::open(&db_path).await.expect("state runtime"),
            cwd,
            session: "session".to_string(),
            config_path: None,
            model: Some("mock/model".to_string()),
            reasoning_effort: None,
            inherited_env: Some(BTreeMap::from([(
                "PSYCHEVO_HOME".to_string(),
                psychevo_home.display().to_string(),
            )])),
        }
    }

    #[tokio::test]
    async fn auto_compaction_check_uses_configured_usage_threshold() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let cwd = temp.path().join("work");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::write(
            home.join("config.toml"),
            r#"[compression]
threshold_percent = 70
reserve_tokens = 5000
"#,
        )
        .expect("config");
        let options = auto_check_options(home.join("state.db"), cwd, home).await;

        assert!(
            !auto_compaction_due_for_snapshot(&options, &snapshot(69_000, Some(100_000)))
                .expect("below threshold")
        );
        assert!(
            auto_compaction_due_for_snapshot(&options, &snapshot(70_000, Some(100_000)))
                .expect("at threshold")
        );
        assert!(
            !auto_compaction_due_for_snapshot(&options, &snapshot(90_000, None))
                .expect("unbounded")
        );
    }

    #[tokio::test]
    async fn cutpoint_preserves_latest_user() {
        let records = vec![
            record(1, user_text_message("old user")),
            record(2, user_text_message("old assistant context")),
            record(3, user_text_message("latest user task")),
        ];
        let prep = prepare_compaction(&records, None, 1).expect("prepare");
        assert_eq!(prep.first_kept_session_seq, Some(3));
    }

    #[tokio::test]
    async fn cutpoint_keeps_tool_call_parent_for_retained_tool_result() {
        let call = ToolCallBlock {
            id: "call-1".to_string(),
            name: "read".to_string(),
            arguments: json!({}),
            arguments_json: "{}".to_string(),
            arguments_error: None,
            content_index: 0,
            call_index: 0,
        };
        let records = vec![
            record(1, user_text_message("old user")),
            record(
                2,
                Message::Assistant {
                    content: vec![AssistantBlock::ToolCall(call)],
                    timestamp_ms: now_ms(),
                    finish_reason: None,
                    outcome: Outcome::Normal,
                    model: None,
                    provider: None,
                },
            ),
            record(
                3,
                Message::ToolResult {
                    tool_call_id: "call-1".to_string(),
                    tool_name: "read".to_string(),
                    content: "large result".to_string(),
                    is_error: false,
                    timestamp_ms: now_ms(),
                },
            ),
            record(4, user_text_message("latest user")),
        ];
        let first = adjust_for_tool_pairs(&records, 2);
        assert_eq!(records[first].session_seq, 2);
    }

    #[tokio::test]
    async fn repeated_compaction_summarizes_from_previous_kept_boundary() {
        let records = vec![
            record(1, user_text_message("already summarized one")),
            record(2, user_text_message("already summarized two")),
            record(3, user_text_message("previously retained one")),
            record(4, user_text_message("previously retained two")),
            record(5, user_text_message("latest user task")),
        ];
        let previous = previous_compaction(3);
        let prep = prepare_compaction(&records, Some(&previous), 1).expect("prepare");

        assert_eq!(prep.first_kept_session_seq, Some(5));
        assert_eq!(
            prep.messages_to_summarize
                .iter()
                .map(|record| record.session_seq)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
    }

    #[tokio::test]
    async fn compacted_context_projection_uses_checkpoint_without_deleting_transcript() {
        let store = StateRuntime::open(std::path::Path::new(":memory:")).await.expect("store");
        let session = store
            .create_session(std::path::Path::new("."))
            .await.expect("session");
        store
            .append_message(&session, &user_text_message("old one"))
            .await.expect("append");
        store
            .append_message(&session, &user_text_message("old two"))
            .await.expect("append");
        store
            .append_message(&session, &user_text_message("latest task"))
            .await.expect("append");
        store
            .append_session_compaction(SessionCompactionInput {
                session_id: session.clone(),
                reason: "manual".to_string(),
                summary_text: "summary text".to_string(),
                first_kept_session_seq: 3,
                created_after_session_seq: 3,
                tokens_before: Some(30),
                tokens_after: Some(10),
                summary_provider: "mock".to_string(),
                summary_model: "mock-model".to_string(),
                instructions: None,
                metadata: None,
            })
            .await.expect("checkpoint");

        let projected = load_projected_messages(&store, &session, None).await.expect("projected");
        assert_eq!(projected.len(), 2);
        assert!(
            serde_json::to_string(&projected[0])
                .expect("summary json")
                .contains("summary text")
        );
        assert!(
            serde_json::to_string(&projected[1])
                .expect("latest json")
                .contains("latest task")
        );
        assert_eq!(
            store.load_message_records(&session).await.expect("records").len(),
            3
        );
    }

    #[tokio::test]
    async fn compacted_context_applies_the_checkpoint_boundary_in_sql() {
        let store = StateRuntime::open(std::path::Path::new(":memory:"))
            .await
            .expect("store");
        let session = store
            .create_session(std::path::Path::new("."))
            .await
            .expect("session");
        store
            .append_message(&session, &user_text_message("already summarized"))
            .await
            .expect("old message");
        store
            .append_message(&session, &user_text_message("retained"))
            .await
            .expect("retained message");
        store
            .append_session_compaction(SessionCompactionInput {
                session_id: session.clone(),
                reason: "manual".to_string(),
                summary_text: "summary text".to_string(),
                first_kept_session_seq: 2,
                created_after_session_seq: 2,
                tokens_before: Some(20),
                tokens_after: Some(10),
                summary_provider: "mock".to_string(),
                summary_model: "mock-model".to_string(),
                instructions: None,
                metadata: None,
            })
            .await
            .expect("checkpoint");
        let mut conn = store.acquire_sqlx().await.expect("connection");
        sqlx::query(
            "UPDATE messages SET message_json = 'not-json' \
             WHERE session_id = ?1 AND session_seq < 2",
        )
        .bind(&session)
        .execute(&mut *conn)
        .await
        .expect("corrupt compacted-away payload");
        drop(conn);

        let projected = load_projected_messages(&store, &session, None)
            .await
            .expect("projected");

        assert_eq!(projected.len(), 2);
        assert!(
            serde_json::to_string(&projected[1])
                .expect("retained json")
                .contains("retained")
        );
    }
}
