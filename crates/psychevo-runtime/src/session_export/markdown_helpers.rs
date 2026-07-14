#[allow(unused_imports)]
use super::*;

pub(crate) fn sanitize_reasoning_for_export(message: &Message) -> Message {
    match message {
        Message::Assistant {
            content,
            timestamp_ms,
            finish_reason,
            outcome,
            model,
            provider,
        } => Message::Assistant {
            content: content
                .iter()
                .filter_map(|block| match block {
                    AssistantBlock::Reasoning { text, .. } if !text.trim().is_empty() => {
                        Some(AssistantBlock::Reasoning {
                            text: text.clone(),
                            provider_evidence: None,
                        })
                    }
                    AssistantBlock::Reasoning { .. } => None,
                    other => Some(other.clone()),
                })
                .collect(),
            timestamp_ms: *timestamp_ms,
            finish_reason: finish_reason.clone(),
            outcome: *outcome,
            model: model.clone(),
            provider: provider.clone(),
        },
        other => other.clone(),
    }
}

pub(crate) fn user_content_markdown(content: &[UserContentBlock]) -> String {
    let mut image_index = 0usize;
    content
        .iter()
        .map(|block| match block {
            UserContentBlock::Text(block) => block.text.clone(),
            UserContentBlock::LocalImage(block) => {
                image_index += 1;
                format!("[Image {image_index}: {}]", block.path.display())
            }
            UserContentBlock::ImageUrl(block) => {
                image_index += 1;
                format!("[Image {image_index}: {}]", block.url)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn push_fenced_json(out: &mut String, value: &Value) {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    push_line(out, "```json");
    push_line(out, &text);
    push_line(out, "```");
}

pub(crate) fn push_fenced_text(out: &mut String, text: &str) {
    push_line(out, "```text");
    push_line(out, text);
    push_line(out, "```");
}

pub(crate) fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

pub(crate) fn markdown_inline(value: &str) -> String {
    value.replace('`', "\\`")
}

pub(crate) fn short_session_id(session_id: &str) -> &str {
    session_id.get(..13).unwrap_or(session_id)
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;
    use psychevo_agent_core::{ToolCallBlock, user_text_message};
    use psychevo_ai::Outcome;
    use tempfile::TempDir;

    use crate::store::{AgentMailboxEventInput, PromptPrefixSlotRecord};

    #[test]
    fn default_export_filename_distinguishes_sibling_uuidv7_sessions() {
        let parent = default_session_export_filename(
            "019e3716-eeb0-7102-9e7b-7a66ac5dc0a1",
            SessionExportFormat::Json,
            SessionArtifactKind::Export,
        );
        let child = default_session_export_filename(
            "019e3716-fa89-7240-9397-1c4a74d8cebf",
            SessionExportFormat::Json,
            SessionArtifactKind::Export,
        );
        assert_ne!(parent, child);
        assert_eq!(parent, "psychevo-session-019e3716-eeb0.json");
        assert_eq!(child, "psychevo-session-019e3716-fa89.json");
    }

    #[test]
    fn export_last_provider_response_uses_persisted_assistant_projection() {
        let tmp = TempDir::new().expect("tmp");
        let db = tmp.path().join("state.db");
        let store = SqliteStore::open(&db).expect("store");
        let session = store
            .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
            .expect("session");
        store
            .append_message(&session, &user_text_message("first prompt"))
            .expect("append user");
        store
            .append_message_with_metrics(
                &session,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "first answer".to_string(),
                    }],
                    timestamp_ms: 2,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("old-model".to_string()),
                    provider: Some("old-provider".to_string()),
                },
                Some(serde_json::json!({"input_tokens": 1})),
                Some(serde_json::json!({"provider_response_id": "resp_old"})),
            )
            .expect("append first assistant");
        store
            .append_message_with_metrics(
                &session,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "latest answer".to_string(),
                    }],
                    timestamp_ms: 3,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("mock-model".to_string()),
                    provider: Some("mock".to_string()),
                },
                Some(serde_json::json!({"input_tokens": 2, "output_tokens": 3})),
                Some(serde_json::json!({"provider_response_id": "resp_latest"})),
            )
            .expect("append latest assistant");

        let artifact = render_session_export(
            &store,
            &session,
            SessionExportOptions {
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::from_values([
                    SessionExportInclude::LastProviderResponse,
                ]),
                artifact_kind: SessionArtifactKind::Export,
            },
        )
        .expect("export");
        let value: Value = serde_json::from_str(&artifact.content).expect("json");
        assert!(value.get("messages").is_none());
        let response = &value["last_provider_response"];
        assert_eq!(response["assistant_session_seq"], 3);
        assert_eq!(response["provider"], "mock");
        assert_eq!(response["model"], "mock-model");
        assert_eq!(response["raw"], false);
        assert_eq!(response["reconstructed"], true);
        assert_eq!(response["source"], "persisted_assistant_message");
        assert_eq!(
            response["warnings"][0],
            "Original provider response chunks are not persisted."
        );
        assert_eq!(response["message"]["content"][0]["text"], "latest answer");
        assert_eq!(response["usage"]["input_tokens"], 2);
        assert_eq!(response["metadata"]["provider_response_id"], "resp_latest");
    }

    #[test]
    fn export_last_provider_response_respects_reasoning_include_policy() {
        let tmp = TempDir::new().expect("tmp");
        let db = tmp.path().join("state.db");
        let store = SqliteStore::open(&db).expect("store");
        let session = store
            .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
            .expect("session");
        store
            .append_message_with_metrics(
                &session,
                &Message::Assistant {
                    content: vec![
                        AssistantBlock::Reasoning {
                            text: "private chain".to_string(),
                            provider_evidence: Some(serde_json::json!({"raw": true})),
                        },
                        AssistantBlock::Text {
                            text: "visible answer".to_string(),
                        },
                    ],
                    timestamp_ms: 1,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
                None,
                None,
            )
            .expect("append assistant");

        let without_reasoning = render_session_export(
            &store,
            &session,
            SessionExportOptions {
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::from_values([
                    SessionExportInclude::LastProviderResponse,
                ]),
                artifact_kind: SessionArtifactKind::Export,
            },
        )
        .expect("export without reasoning");
        let value: Value = serde_json::from_str(&without_reasoning.content).expect("json");
        let content = value["last_provider_response"]["message"]["content"]
            .as_array()
            .expect("content");
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");

        let with_reasoning = render_session_export(
            &store,
            &session,
            SessionExportOptions {
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::new(
                    [
                        SessionExportInclude::Reasoning,
                        SessionExportInclude::LastProviderResponse,
                    ],
                    SessionArtifactKind::Export,
                )
                .expect("include"),
                artifact_kind: SessionArtifactKind::Export,
            },
        )
        .expect("export with reasoning");
        let value: Value = serde_json::from_str(&with_reasoning.content).expect("json");
        let content = value["last_provider_response"]["message"]["content"]
            .as_array()
            .expect("content");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "reasoning");
        assert_eq!(content[0]["text"], "private chain");
        assert!(content[0].get("provider_evidence").is_none());
    }

    #[test]
    fn export_last_provider_request_omits_tools_for_empty_effective_policy() {
        let tmp = TempDir::new().expect("tmp");
        let db = tmp.path().join("state.db");
        let store = SqliteStore::open(&db).expect("store");
        let session = store
            .create_session_with_metadata(
                tmp.path(),
                "run",
                "model",
                "provider",
                Some(serde_json::json!({
                    "base_url": "https://example.test/v1",
                    "mode": "default",
                    "model_metadata": {
                        "capabilities": {
                            "tool_call": true
                        }
                    }
                })),
            )
            .expect("session");
        let prefix_hash = "empty-tools-prefix";
        let prompt_prefix_metadata = serde_json::json!({
            "prompt_prefix": {
                "hash": prefix_hash,
                "version": 1,
                "created_at_ms": 1,
                "provider": "provider",
                "model": "model",
                "tool_declarations_hash": "empty-tools-hash",
                "invalidation_reason": "new_session",
                "effective_tools": [],
                "agent_catalog_visible": false,
                "visible_agents": [],
                "skill_catalog_visible": false,
                "project_instructions_visible": false,
                "project_instructions_role": null
            }
        });
        store
            .append_message_with_undo_snapshot_metadata_and_context_evidence(
                &session,
                &user_text_message("translate this"),
                Some(prompt_prefix_metadata),
                None,
                &[],
            )
            .expect("append user");
        store
            .append_message(
                &session,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "translated".to_string(),
                    }],
                    timestamp_ms: 2,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
            )
            .expect("append assistant");
        store
            .upsert_session_prompt_prefix(PromptPrefixRecord {
                session_id: session.clone(),
                version: 0,
                created_at_ms: 1,
                provider: "provider".to_string(),
                model: "model".to_string(),
                prefix_hash: prefix_hash.to_string(),
                tool_declarations_hash: "empty-tools-hash".to_string(),
                invalidation_reason: Some("new_session".to_string()),
                slots: vec![PromptPrefixSlotRecord {
                    slot: "base/mode".to_string(),
                    tier: "base".to_string(),
                    semantic_role: "base_policy".to_string(),
                    provider_role: "system".to_string(),
                    order: 0,
                    content: "Runtime mode: default. No callable tools are available.".to_string(),
                    content_hash: "base".to_string(),
                    source_kind: Some("runtime".to_string()),
                    source_name: Some("mode".to_string()),
                    source_path: None,
                }],
                metadata: Some(serde_json::json!({
                    "mode": "default",
                    "selected_agent": null,
                    "agents_enabled": true,
                    "effective_tools": [],
                    "agent_catalog_visible": false,
                    "visible_agents": [],
                    "skill_catalog_visible": false,
                    "project_instructions_visible": false,
                    "project_instructions_role": null
                })),
            })
            .expect("prefix");

        let artifact = render_session_export(
            &store,
            &session,
            SessionExportOptions {
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::from_values([
                    SessionExportInclude::Header,
                    SessionExportInclude::LastProviderRequest,
                ]),
                artifact_kind: SessionArtifactKind::Export,
            },
        )
        .expect("export");
        let value: Value = serde_json::from_str(&artifact.content).expect("json");
        assert_eq!(
            value["header"]["prompt_prefix"]["metadata"]["effective_tools"],
            serde_json::json!([])
        );
        assert!(
            value["last_provider_request"]["body"]
                .as_object()
                .expect("body")
                .get("tools")
                .is_none()
        );
    }

    #[test]
    fn export_last_provider_request_uses_message_prompt_prefix_version() {
        let tmp = TempDir::new().expect("tmp");
        let db = tmp.path().join("state.db");
        let store = SqliteStore::open(&db).expect("store");
        let session = store
            .create_session_with_metadata(
                tmp.path(),
                "run",
                "model",
                "provider",
                Some(serde_json::json!({
                    "base_url": "https://example.test/v1",
                    "mode": "default",
                    "model_metadata": {
                        "capabilities": {
                            "tool_call": true
                        }
                    }
                })),
            )
            .expect("session");
        store
            .upsert_session_prompt_prefix(PromptPrefixRecord {
                session_id: session.clone(),
                version: 0,
                created_at_ms: 1,
                provider: "provider".to_string(),
                model: "model".to_string(),
                prefix_hash: "old-prefix".to_string(),
                tool_declarations_hash: "old-tools-hash".to_string(),
                invalidation_reason: Some("new_session".to_string()),
                slots: vec![PromptPrefixSlotRecord {
                    slot: "base/mode".to_string(),
                    tier: "base".to_string(),
                    semantic_role: "base_policy".to_string(),
                    provider_role: "system".to_string(),
                    order: 0,
                    content: "old prefix content".to_string(),
                    content_hash: "old".to_string(),
                    source_kind: Some("runtime".to_string()),
                    source_name: Some("mode".to_string()),
                    source_path: None,
                }],
                metadata: Some(serde_json::json!({
                    "mode": "default",
                    "effective_tools": []
                })),
            })
            .expect("old prefix");
        store
            .upsert_session_prompt_prefix(PromptPrefixRecord {
                session_id: session.clone(),
                version: 0,
                created_at_ms: 2,
                provider: "provider".to_string(),
                model: "model".to_string(),
                prefix_hash: "new-prefix".to_string(),
                tool_declarations_hash: "new-tools-hash".to_string(),
                invalidation_reason: Some("runtime_context_changed".to_string()),
                slots: vec![PromptPrefixSlotRecord {
                    slot: "base/mode".to_string(),
                    tier: "base".to_string(),
                    semantic_role: "base_policy".to_string(),
                    provider_role: "system".to_string(),
                    order: 0,
                    content: "new prefix content".to_string(),
                    content_hash: "new".to_string(),
                    source_kind: Some("runtime".to_string()),
                    source_name: Some("mode".to_string()),
                    source_path: None,
                }],
                metadata: Some(serde_json::json!({
                    "mode": "default",
                    "effective_tools": []
                })),
            })
            .expect("new prefix");
        store
            .append_message_with_undo_snapshot_metadata_and_context_evidence(
                &session,
                &user_text_message("use old prefix"),
                Some(serde_json::json!({
                    "prompt_prefix": {
                        "hash": "old-prefix",
                        "version": 1,
                        "created_at_ms": 1,
                        "provider": "provider",
                        "model": "model",
                        "tool_declarations_hash": "old-tools-hash",
                        "effective_tools": []
                    }
                })),
                None,
                &[],
            )
            .expect("append user");
        store
            .append_message(
                &session,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "done".to_string(),
                    }],
                    timestamp_ms: 3,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
            )
            .expect("append assistant");

        let artifact = render_session_export(
            &store,
            &session,
            SessionExportOptions {
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::from_values([
                    SessionExportInclude::Header,
                    SessionExportInclude::LastProviderRequest,
                ]),
                artifact_kind: SessionArtifactKind::Export,
            },
        )
        .expect("export");
        let value: Value = serde_json::from_str(&artifact.content).expect("json");
        assert_eq!(
            value["header"]["prompt_prefix"]["prefix_hash"],
            serde_json::json!("new-prefix")
        );
        let body_text =
            serde_json::to_string(&value["last_provider_request"]["body"]).expect("body");
        assert!(body_text.contains("old prefix content"));
        assert!(!body_text.contains("new prefix content"));
    }

    #[test]
    fn export_last_provider_request_reconstructs_clarify_declaration() {
        let tmp = TempDir::new().expect("tmp");
        let db = tmp.path().join("state.db");
        let store = SqliteStore::open(&db).expect("store");
        let session = store
            .create_session_with_metadata(
                tmp.path(),
                "tui",
                "model",
                "provider",
                Some(serde_json::json!({
                    "base_url": "https://example.test/v1",
                    "mode": "plan",
                    "model_metadata": {
                        "capabilities": {
                            "tool_call": true
                        }
                    }
                })),
            )
            .expect("session");
        let prefix_hash = "clarify-tools-prefix";
        let prompt_prefix_metadata = serde_json::json!({
            "prompt_prefix": {
                "hash": prefix_hash,
                "version": 1,
                "created_at_ms": 1,
                "provider": "provider",
                "model": "model",
                "tool_declarations_hash": "clarify-tools-hash",
                "invalidation_reason": "new_session",
                "effective_tools": ["clarify"],
                "agent_catalog_visible": false,
                "visible_agents": [],
                "skill_catalog_visible": false,
                "project_instructions_visible": false,
                "project_instructions_role": null
            }
        });
        store
            .append_message_with_undo_snapshot_metadata_and_context_evidence(
                &session,
                &user_text_message("ask before proceeding"),
                Some(prompt_prefix_metadata),
                None,
                &[],
            )
            .expect("append user");
        store
            .append_message(
                &session,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "I will ask.".to_string(),
                    }],
                    timestamp_ms: 2,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
            )
            .expect("append assistant");
        store
            .upsert_session_prompt_prefix(PromptPrefixRecord {
                session_id: session.clone(),
                version: 0,
                created_at_ms: 1,
                provider: "provider".to_string(),
                model: "model".to_string(),
                prefix_hash: prefix_hash.to_string(),
                tool_declarations_hash: "clarify-tools-hash".to_string(),
                invalidation_reason: Some("new_session".to_string()),
                slots: vec![PromptPrefixSlotRecord {
                    slot: "base/mode".to_string(),
                    tier: "base".to_string(),
                    semantic_role: "base_policy".to_string(),
                    provider_role: "system".to_string(),
                    order: 0,
                    content: "Runtime mode: plan. Use clarify when needed.".to_string(),
                    content_hash: "base".to_string(),
                    source_kind: Some("runtime".to_string()),
                    source_name: Some("mode".to_string()),
                    source_path: None,
                }],
                metadata: Some(serde_json::json!({
                    "mode": "plan",
                    "selected_agent": null,
                    "agents_enabled": true,
                    "effective_tools": ["clarify"],
                    "agent_catalog_visible": false,
                    "visible_agents": [],
                    "skill_catalog_visible": false,
                    "project_instructions_visible": false,
                    "project_instructions_role": null
                })),
            })
            .expect("prefix");

        let artifact = render_session_export(
            &store,
            &session,
            SessionExportOptions {
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::from_values([
                    SessionExportInclude::Header,
                    SessionExportInclude::LastProviderRequest,
                ]),
                artifact_kind: SessionArtifactKind::Export,
            },
        )
        .expect("export");
        let value: Value = serde_json::from_str(&artifact.content).expect("json");
        assert_eq!(
            value["header"]["prompt_prefix"]["metadata"]["effective_tools"],
            serde_json::json!(["clarify"])
        );
        let tools = value["last_provider_request"]["body"]["tools"]
            .as_array()
            .expect("tools");
        assert_eq!(tools.len(), 1);
        let clarify = tools
            .iter()
            .find(|tool| tool["function"]["name"] == "clarify")
            .expect("clarify declaration");
        assert_eq!(
            clarify["function"]["parameters"]["properties"]["questions"]["maxItems"],
            serde_json::json!(3)
        );
        assert_eq!(
            clarify["function"]["parameters"]["properties"]["questions"]["items"]["properties"]["options"]
                ["maxItems"],
            serde_json::json!(3)
        );
        let question_properties =
            clarify["function"]["parameters"]["properties"]["questions"]["items"]["properties"]
                .as_object()
                .expect("question properties");
        assert!(question_properties.contains_key("question"));
        assert!(question_properties.contains_key("options"));
        assert!(!question_properties.contains_key("id"));
        assert!(!question_properties.contains_key("header"));
    }

    #[test]
    fn export_last_provider_request_includes_mailbox_result_once_after_wait() {
        let tmp = TempDir::new().expect("tmp");
        let db = tmp.path().join("state.db");
        let store = SqliteStore::open(&db).expect("store");
        let session = store
            .create_session_with_metadata(
                tmp.path(),
                "run",
                "model",
                "provider",
                Some(serde_json::json!({
                    "base_url": "https://example.test/v1",
                    "mode": "default",
                    "model_metadata": {
                        "capabilities": {
                            "tool_call": true
                        }
                    }
                })),
            )
            .expect("session");
        let prefix_hash = "mailbox-prefix";
        let prompt_prefix_metadata = serde_json::json!({
            "prompt_prefix": {
                "hash": prefix_hash,
                "version": 1,
                "created_at_ms": 1,
                "provider": "provider",
                "model": "model",
                "tool_declarations_hash": "mailbox-tools-hash",
                "invalidation_reason": "new_session",
                "effective_tools": ["wait_agent"],
                "agent_catalog_visible": false,
                "visible_agents": [],
                "skill_catalog_visible": false,
                "project_instructions_visible": false,
                "project_instructions_role": null
            }
        });
        store
            .append_message_with_undo_snapshot_metadata_and_context_evidence(
                &session,
                &user_text_message("wait for workers"),
                Some(prompt_prefix_metadata),
                None,
                &[],
            )
            .expect("append user");
        store
            .append_message(
                &session,
                &Message::Assistant {
                    content: vec![AssistantBlock::ToolCall(ToolCallBlock {
                        id: "call-wait".to_string(),
                        name: "wait_agent".to_string(),
                        arguments: serde_json::json!({"timeout_ms": 1000}),
                        arguments_json: "{\"timeout_ms\":1000}".to_string(),
                        arguments_error: None,
                        content_index: 0,
                        call_index: 0,
                    })],
                    timestamp_ms: 2,
                    finish_reason: Some("tool_calls".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
            )
            .expect("append assistant tool call");
        store
            .append_message(
                &session,
                &Message::ToolResult {
                    tool_call_id: "call-wait".to_string(),
                    tool_name: "wait_agent".to_string(),
                    content: "{\"message\":\"Wait completed.\",\"timed_out\":false}".to_string(),
                    is_error: false,
                    timestamp_ms: 3,
                },
            )
            .expect("append wait result");
        let final_answer = "unique mailbox final answer";
        let payload = serde_json::json!({
            "author": "/root/worker",
            "recipient": "/root",
            "other_recipients": [],
            "content": format!(
                "<subagent_notification>\n{}\n</subagent_notification>",
                serde_json::json!({
                    "agent_id": "agent-1",
                    "agent_name": "worker",
                    "status": "completed",
                    "outcome": "normal",
                    "final_answer": final_answer
                })
            ),
            "trigger_turn": false
        });
        store
            .append_agent_mailbox_event(AgentMailboxEventInput {
                parent_session_id: session.clone(),
                child_session_id: None,
                agent_id: "agent-1".to_string(),
                task_name: Some("worker".to_string()),
                agent_name: "worker".to_string(),
                content_text: serde_json::to_string(&payload).expect("payload text"),
                payload,
                metadata: None,
            })
            .expect("mailbox event");
        store
            .deliver_pending_agent_mailbox_events_for_tool(&session, "call-wait", 3)
            .expect("deliver");
        store
            .append_message(
                &session,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: "synthesized".to_string(),
                    }],
                    timestamp_ms: 4,
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("model".to_string()),
                    provider: Some("provider".to_string()),
                },
            )
            .expect("append final assistant");
        store
            .upsert_session_prompt_prefix(PromptPrefixRecord {
                session_id: session.clone(),
                version: 0,
                created_at_ms: 1,
                provider: "provider".to_string(),
                model: "model".to_string(),
                prefix_hash: prefix_hash.to_string(),
                tool_declarations_hash: "mailbox-tools-hash".to_string(),
                invalidation_reason: Some("new_session".to_string()),
                slots: vec![PromptPrefixSlotRecord {
                    slot: "base/mode".to_string(),
                    tier: "base".to_string(),
                    semantic_role: "base_policy".to_string(),
                    provider_role: "system".to_string(),
                    order: 0,
                    content: "Runtime mode: default.".to_string(),
                    content_hash: "base".to_string(),
                    source_kind: Some("runtime".to_string()),
                    source_name: Some("mode".to_string()),
                    source_path: None,
                }],
                metadata: Some(serde_json::json!({
                    "mode": "default",
                    "selected_agent": null,
                    "agents_enabled": true,
                    "effective_tools": ["wait_agent"],
                    "agent_catalog_visible": false,
                    "visible_agents": [],
                    "skill_catalog_visible": false,
                    "project_instructions_visible": false,
                    "project_instructions_role": null
                })),
            })
            .expect("prefix");

        let artifact = render_session_export(
            &store,
            &session,
            SessionExportOptions {
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::from_values([
                    SessionExportInclude::Header,
                    SessionExportInclude::LastProviderRequest,
                ]),
                artifact_kind: SessionArtifactKind::Export,
            },
        )
        .expect("export");
        let value: Value = serde_json::from_str(&artifact.content).expect("json");
        let body = &value["last_provider_request"]["body"];
        let body_text = serde_json::to_string(body).expect("body text");
        assert_eq!(body_text.matches(final_answer).count(), 1);
        let wait_tool_result = body["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .find(|message| {
                message.get("role").and_then(Value::as_str) == Some("tool")
                    && message.get("tool_call_id").and_then(Value::as_str) == Some("call-wait")
            })
            .expect("wait tool result");
        assert!(
            !wait_tool_result
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains(final_answer)
        );
    }
}
