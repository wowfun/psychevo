#[allow(unused_imports)]
use super::*;

#[test]
pub(crate) fn streaming_tool_completion_reuses_pending_row_as_completed_evidence() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_update",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_write",
                    "name": "write",
                    "arguments": {"path": "report.md", "content": "body"},
                    "arguments_json": "{\"path\":\"report.md\",\"content\":\"body\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }]
            }
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call_write",
            "tool_name": "write",
            "result": {"path": "report.md", "bytes_written": 4},
            "outcome": "normal",
            "elapsed_ms": 65_000
        }),
        false,
    );

    assert_eq!(ui.transcript.len(), 1);
    let row = &ui.transcript[0];
    assert_eq!(row.kind, TranscriptKind::Updated);
    assert_eq!(row.title, "write report.md");
    assert!(row.text.contains("write normal"));
    assert_eq!(row.tool_elapsed, Some(Duration::from_millis(65_000)));
    assert!(row.tool_started.is_none());
}

#[test]
pub(crate) fn agent_pending_row_with_position_id_upgrade_merges_into_resolved_child_row() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let agent_args = serde_json::json!({
        "agent_type": "translate",
        "task_name": "en-to-cn",
        "prompt": "Please translate this English text to Chinese."
    });

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_call_id": "provisional-agent-id",
            "tool_name": "spawn_agent",
            "arguments": agent_args,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    assert_eq!(agent_rows(&ui).len(), 1);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "resolved-agent-id",
            "tool_name": "spawn_agent",
            "args": agent_args,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_session_start",
            "tool_call_id": "resolved-agent-id",
            "agent_name": "translate",
            "task_name": "en-to-cn",
            "child_session_id": "child-en-to-cn"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "resolved-agent-id",
            "tool_name": "spawn_agent",
            "args": agent_args,
            "result": {
                "agent_name": "translate",
                "task_name": "en-to-cn",
                "child_session_id": "child-en-to-cn",
                "child_session": {"tool_call_count": 0, "latest_total_tokens": 864},
                "status": "completed",
                "summary": "译文"
            },
            "outcome": "normal",
            "elapsed_ms": 2_000
        }),
        false,
    );
    ui.finish_turn();

    let rows = agent_rows(&ui);
    assert_eq!(rows.len(), 1, "{:#?}", ui.transcript);
    let row = rows[0];
    assert_eq!(row.tool_call_id.as_deref(), Some("resolved-agent-id"));
    assert_eq!(row.agent_target.as_deref(), Some("child-en-to-cn"));
    assert!(!row.interrupted, "{row:#?}");
    assert_eq!(row.text, "Done (0 tool uses · 864 tokens)");
}

#[test]
pub(crate) fn parallel_agent_pending_rows_match_by_position_not_agent_name() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let cn_args = serde_json::json!({
        "agent_type": "translate",
        "task_name": "cn-to-en",
        "prompt": "请把这段中文翻译为英文。"
    });
    let en_args = serde_json::json!({
        "agent_type": "translate",
        "task_name": "en-to-cn",
        "prompt": "Please translate this English text to Chinese."
    });

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": cn_args,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": en_args,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "resolved-en",
            "tool_name": "spawn_agent",
            "args": en_args,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );
    assert!(
        ui.transcript.iter().any(|row| {
            row.tool_call_id.as_deref() == Some("resolved-en") && row.title == "translate(en-to-cn)"
        }),
        "{:#?}",
        ui.transcript
    );
    assert!(
        ui.transcript
            .iter()
            .any(|row| row.tool_call_id.is_none() && row.title == "translate(cn-to-en)"),
        "{:#?}",
        ui.transcript
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "resolved-en",
            "tool_name": "spawn_agent",
            "args": en_args,
            "result": {
                "agent_name": "translate",
                "task_name": "en-to-cn",
                "child_session_id": "child-en",
                "child_session": {"tool_call_count": 0, "latest_total_tokens": 864},
                "status": "completed",
                "summary": "译文"
            },
            "outcome": "normal"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "resolved-cn",
            "tool_name": "spawn_agent",
            "args": cn_args,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "resolved-cn",
            "tool_name": "spawn_agent",
            "args": cn_args,
            "result": {
                "agent_name": "translate",
                "task_name": "cn-to-en",
                "child_session_id": "child-cn",
                "child_session": {"tool_call_count": 0, "latest_total_tokens": 840},
                "status": "completed",
                "summary": "translation"
            },
            "outcome": "normal"
        }),
        false,
    );
    ui.finish_turn();

    let rows = agent_rows(&ui);
    assert_eq!(rows.len(), 2, "{:#?}", ui.transcript);
    assert!(rows.iter().all(|row| !row.interrupted), "{rows:#?}");
    assert!(
        rows.iter()
            .any(|row| row.agent_target.as_deref() == Some("child-en"))
    );
    assert!(
        rows.iter()
            .any(|row| row.agent_target.as_deref() == Some("child-cn"))
    );
}

#[test]
pub(crate) fn late_agent_pending_after_completion_does_not_create_third_row() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let cn_args = serde_json::json!({
        "agent_type": "translate",
        "task_name": "cn-to-en",
        "prompt": "请翻译以下内容：人工智能正在深刻改变我们的生活方式。"
    });
    let en_args = serde_json::json!({
        "agent_type": "translate",
        "task_name": "en-to-cn",
        "prompt": "请翻译以下内容：The art of programming is the art of organizing complexity."
    });

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": cn_args,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": en_args,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [],
                "timestamp_ms": 1,
                "finish_reason": "tool_calls",
                "outcome": "normal"
            }
        }),
        false,
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call-cn",
            "tool_name": "spawn_agent",
            "args": cn_args,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_session_start",
            "tool_call_id": "call-cn",
            "agent_name": "translate",
            "task_name": "cn-to-en",
            "child_session_id": "child-cn"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-cn",
            "tool_name": "spawn_agent",
            "args": cn_args,
            "result": {
                "agent_name": "translate",
                "task_name": "cn-to-en",
                "child_session_id": "child-cn",
                "child_session": {"tool_call_count": 0, "latest_total_tokens": 794},
                "status": "completed",
                "summary": "Artificial intelligence is changing our lives."
            },
            "outcome": "normal"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call-en",
            "tool_name": "spawn_agent",
            "args": en_args,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );

    let rows = agent_rows(&ui);
    assert_eq!(rows.len(), 2, "{:#?}", ui.transcript);
    assert!(
        rows.iter()
            .any(|row| row.agent_target.as_deref() == Some("child-cn"))
    );
    let en_row = rows
        .iter()
        .find(|row| row.tool_call_id.as_deref() == Some("call-en"))
        .expect("resolved en row should adopt weak placeholder");
    assert!(active_tool_row(en_row), "{en_row:#?}");
    assert_eq!(en_row.agent_target, None);
    assert_ne!(en_row.text, "interrupted");

    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_session_start",
            "tool_call_id": "call-en",
            "agent_name": "translate",
            "task_name": "en-to-cn",
            "child_session_id": "child-en"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-en",
            "tool_name": "spawn_agent",
            "args": en_args,
            "result": {
                "agent_name": "translate",
                "task_name": "en-to-cn",
                "child_session_id": "child-en",
                "child_session": {"tool_call_count": 0, "latest_total_tokens": 794},
                "status": "completed",
                "summary": "编程的艺术是组织复杂性的艺术。"
            },
            "outcome": "normal"
        }),
        false,
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": en_args,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );

    let rows = agent_rows(&ui);
    assert_eq!(rows.len(), 2, "{:#?}", ui.transcript);
    assert!(rows.iter().all(|row| !active_tool_row(row)), "{rows:#?}");
    assert!(
        rows.iter()
            .any(|row| row.agent_target.as_deref() == Some("child-cn"))
    );
    assert!(
        rows.iter()
            .any(|row| row.agent_target.as_deref() == Some("child-en"))
    );
}

#[test]
pub(crate) fn weak_agent_placeholder_with_only_agent_type_adopts_resolved_position() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let cn_args = serde_json::json!({
        "agent_type": "translate",
        "prompt": "请将以下中文翻译成英文：\"今天天气真好，我们去公园散步吧。\""
    });
    let en_args = serde_json::json!({
        "agent_type": "translate",
        "prompt": "请将以下英文翻译成中文：\"The quick brown fox jumps over the lazy dog.\""
    });

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": cn_args,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call-cn",
            "tool_name": "spawn_agent",
            "args": cn_args,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_session_start",
            "tool_call_id": "call-cn",
            "agent_name": "translate",
            "task_name": "translate-1",
            "child_session_id": "child-cn"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-cn",
            "tool_name": "spawn_agent",
            "args": cn_args,
            "result": {
                "agent_name": "translate",
                "task": "请将以下中文翻译成英文：",
                "child_session_id": "child-cn",
                "child_session": {"tool_call_count": 0, "latest_total_tokens": 782},
                "status": "completed",
                "summary": "The weather is really nice today."
            },
            "outcome": "normal"
        }),
        false,
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": {"agent_type": "translate"},
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_start",
            "tool_call_id": "call-en",
            "tool_name": "spawn_agent",
            "args": en_args,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_session_start",
            "tool_call_id": "call-en",
            "agent_name": "translate",
            "task_name": "translate-2",
            "child_session_id": "child-en"
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-en",
            "tool_name": "spawn_agent",
            "args": en_args,
            "result": {
                "agent_name": "translate",
                "task": "请将以下英文翻译成中文：",
                "child_session_id": "child-en",
                "child_session": {"tool_call_count": 0, "latest_total_tokens": 869},
                "status": "completed",
                "summary": "那只敏捷的棕色狐狸跳过了那只懒狗。"
            },
            "outcome": "normal"
        }),
        false,
    );
    ui.finish_turn();

    let rows = agent_rows(&ui);
    assert_eq!(rows.len(), 2, "{:#?}", ui.transcript);
    assert!(rows.iter().all(|row| !row.interrupted), "{rows:#?}");
    assert!(
        rows.iter()
            .any(|row| row.agent_target.as_deref() == Some("child-cn"))
    );
    assert!(
        rows.iter()
            .any(|row| row.agent_target.as_deref() == Some("child-en"))
    );
}

#[test]
pub(crate) fn agent_session_start_with_unknown_id_does_not_steal_pending_row() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let cn_args = serde_json::json!({
        "agent_type": "translate",
        "task_name": "cn-to-en",
        "prompt": "请把这段中文翻译为英文。"
    });
    let en_args = serde_json::json!({
        "agent_type": "translate",
        "task_name": "en-to-cn",
        "prompt": "Please translate this English text to Chinese."
    });

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": cn_args,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": en_args,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_session_start",
            "tool_call_id": "resolved-en",
            "agent_name": "translate",
            "task_name": "en-to-cn",
            "child_session_id": "child-en"
        }),
        false,
    );

    let resolved_en_row = ui
        .transcript
        .iter()
        .find(|row| row.tool_call_id.as_deref() == Some("resolved-en"))
        .expect("resolved en row");
    assert_eq!(resolved_en_row.agent_target.as_deref(), Some("child-en"));
    let cn_row = ui
        .transcript
        .iter()
        .find(|row| row.title == "translate(cn-to-en)")
        .expect("cn row");
    assert!(cn_row.agent_target.is_none(), "{cn_row:#?}");
    assert_eq!(agent_rows(&ui).len(), 3, "{:#?}", ui.transcript);
}

#[test]
pub(crate) fn background_agent_handoff_keeps_single_row_for_late_partial_pending() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let cn_pending = serde_json::json!({
        "agent_type": "translate",
        "task_name": "translate-zh-to-en"
    });
    let en_pending = serde_json::json!({
        "agent_type": "translate",
        "task_name": "translate-en-to-zh"
    });
    let cn_prompt = "请翻译以下中文为英文：人工智能正在深刻改变我们的生活方式。";
    let en_prompt = "Please translate the following English text into Chinese.";

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": cn_pending,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": en_pending,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-cn",
            "tool_name": "spawn_agent",
            "content_index": 0,
            "call_index": 0,
            "result": {
                "agent_name": "translate",
                "agent_description": "Detect the source language automatically. Translate Chinese to English; translate all other languages to Chinese.",
                "task_name": "translate-zh-to-en",
                "task": cn_prompt,
                "status": "running",
                "background": true,
                "session_id": "child-cn",
                "child_session_id": "child-cn"
            },
            "outcome": "normal",
            "elapsed_ms": 13
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-en",
            "tool_name": "spawn_agent",
            "content_index": 1,
            "call_index": 1,
            "result": {
                "agent_name": "translate",
                "agent_description": "Detect the source language automatically. Translate Chinese to English; translate all other languages to Chinese.",
                "task_name": "translate-en-to-zh",
                "task": en_prompt,
                "status": "running",
                "background": true,
                "session_id": "child-en",
                "child_session_id": "child-en"
            },
            "outcome": "normal",
            "elapsed_ms": 24
        }),
        false,
    );

    let rows = agent_rows(&ui);
    assert_eq!(rows.len(), 2, "{:#?}", ui.transcript);
    assert!(
        rows.iter()
            .any(|row| row.agent_target.as_deref() == Some("child-cn"))
    );
    assert!(
        rows.iter()
            .any(|row| row.agent_target.as_deref() == Some("child-en"))
    );
    assert!(rows.iter().all(|row| !row.interrupted), "{rows:#?}");

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": en_pending,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );
    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_session_start",
            "tool_call_id": "call-en",
            "agent_name": "translate",
            "agent_description": "Detect the source language automatically. Translate Chinese to English; translate all other languages to Chinese.",
            "task_name": "translate-en-to-zh",
            "task": en_prompt,
            "child_session_id": "child-en",
            "background": true
        }),
        false,
    );

    let rows = agent_rows(&ui);
    assert_eq!(rows.len(), 2, "{:#?}", ui.transcript);
    assert!(rows.iter().all(|row| !row.interrupted), "{rows:#?}");
    let en_row = rows
        .iter()
        .find(|row| row.agent_target.as_deref() == Some("child-en"))
        .expect("en row");
    assert_eq!(en_row.tool_call_id.as_deref(), Some("call-en"));
    assert_eq!(en_row.title, "translate(translate-en-to-zh)");
    assert_eq!(en_row.text, "Started in background");
}

#[test]
pub(crate) fn background_agent_handoff_stepwise_never_duplicates_or_interrupts() {
    let temp = tempdir().expect("temp");
    let app = test_app(&temp);
    let mut ui = FullscreenUi::new(&app);
    let cn_args = serde_json::json!({
        "agent_type": "translate",
        "task_name": "cn_to_en",
        "message": "Translate this Chinese sentence to English."
    });
    let en_args = serde_json::json!({
        "agent_type": "translate",
        "task_name": "en_to_cn",
        "message": "Translate this English sentence to Chinese."
    });

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": cn_args,
            "content_index": 0,
            "call_index": 0
        }),
        false,
    );
    assert_stable_agent_rows(&ui, 1);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": en_args,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );
    assert_stable_agent_rows(&ui, 2);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-cn",
            "tool_name": "spawn_agent",
            "content_index": 0,
            "call_index": 0,
            "result": {
                "agent_name": "translate",
                "task_name": "cn_to_en",
                "status": "running",
                "background": true,
                "child_session_id": "child-cn"
            },
            "outcome": "normal",
            "elapsed_ms": 8
        }),
        false,
    );
    assert_stable_agent_rows(&ui, 2);
    assert_agent_row_target(&ui, "call-cn", Some("child-cn"));
    assert_agent_row_target(&ui, "", None);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "agent_session_start",
            "tool_call_id": "call-cn",
            "agent_name": "translate",
            "task_name": "cn_to_en",
            "child_session_id": "child-cn"
        }),
        false,
    );
    assert_stable_agent_rows(&ui, 2);

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_execution_end",
            "tool_call_id": "call-en",
            "tool_name": "spawn_agent",
            "content_index": 1,
            "call_index": 1,
            "result": {
                "agent_name": "translate",
                "task_name": "en_to_cn",
                "status": "running",
                "background": true,
                "child_session_id": "child-en"
            },
            "outcome": "normal",
            "elapsed_ms": 11
        }),
        false,
    );
    assert_stable_agent_rows(&ui, 2);
    assert_agent_row_target(&ui, "call-en", Some("child-en"));

    ui.apply_value_event(
        &serde_json::json!({
            "type": "tool_call_pending",
            "tool_name": "spawn_agent",
            "arguments": en_args,
            "content_index": 1,
            "call_index": 1
        }),
        false,
    );
    assert_stable_agent_rows(&ui, 2);
    assert_agent_row_target(&ui, "call-en", Some("child-en"));

    ui.turn_outcome = Some(Outcome::Normal);
    ui.finish_turn();
    assert_stable_agent_rows(&ui, 2);
}
