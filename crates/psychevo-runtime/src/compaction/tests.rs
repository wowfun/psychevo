#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;
    use crate::context_usage::{ContextScope, ContextTokenizer, ContextTotal};
    use psychevo_agent_core::{AssistantBlock, ToolCallBlock, now_ms};
    use std::fs;
    use std::path::PathBuf;

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

    fn auto_check_options(
        db_path: PathBuf,
        workdir: PathBuf,
        psychevo_home: PathBuf,
    ) -> AutoCompactionCheckOptions {
        AutoCompactionCheckOptions {
            state: StateRuntime::open(&db_path).expect("state runtime"),
            workdir,
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

    #[test]
    fn auto_compaction_check_uses_configured_usage_threshold() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let workdir = temp.path().join("work");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&workdir).expect("workdir");
        fs::write(
            home.join("config.toml"),
            r#"[compression]
threshold_percent = 70
reserve_tokens = 5000
"#,
        )
        .expect("config");
        let options = auto_check_options(home.join("state.db"), workdir, home);

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

    #[test]
    fn cutpoint_preserves_latest_user() {
        let records = vec![
            record(1, user_text_message("old user")),
            record(2, user_text_message("old assistant context")),
            record(3, user_text_message("latest user task")),
        ];
        let prep = prepare_compaction(&records, None, 1).expect("prepare");
        assert_eq!(prep.first_kept_session_seq, Some(3));
    }

    #[test]
    fn cutpoint_keeps_tool_call_parent_for_retained_tool_result() {
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

    #[test]
    fn repeated_compaction_summarizes_from_previous_kept_boundary() {
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

    #[test]
    fn compacted_context_projection_uses_checkpoint_without_deleting_transcript() {
        let store = SqliteStore::open(std::path::Path::new(":memory:")).expect("store");
        let session = store
            .create_session(std::path::Path::new("."))
            .expect("session");
        store
            .append_message(&session, &user_text_message("old one"))
            .expect("append");
        store
            .append_message(&session, &user_text_message("old two"))
            .expect("append");
        store
            .append_message(&session, &user_text_message("latest task"))
            .expect("append");
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
            .expect("checkpoint");

        let projected = load_projected_messages(&store, &session, None).expect("projected");
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
            store.load_message_records(&session).expect("records").len(),
            3
        );
    }
}
