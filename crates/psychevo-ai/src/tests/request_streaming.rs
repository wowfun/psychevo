#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn chat_request_maps_local_image_blocks_to_content_parts() {
    let temp = tempfile::tempdir().expect("temp");
    let image = temp.path().join("image.avif");
    std::fs::write(&image, tiny_avif_bytes()).expect("image");
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        },
        messages: vec![json!({
            "role": "user",
            "content": [
                { "type": "local_image", "path": image },
                { "type": "image_url", "url": "https://example.com/image.png" },
                { "text": "describe it" }
            ],
            "timestamp_ms": 1
        })],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");

    let content = body["messages"][0]["content"]
        .as_array()
        .expect("content parts");
    assert_eq!(content.len(), 3);
    assert_eq!(content[0]["type"], "image_url");
    let data_url = content[0]["image_url"]["url"].as_str().expect("data url");
    assert!(
        data_url.starts_with("data:image/png;base64,")
            || data_url.starts_with("data:image/avif;base64,")
    );
    assert_eq!(
        content[1],
        json!({
            "type": "image_url",
            "image_url": { "url": "https://example.com/image.png" }
        })
    );
    assert_eq!(content[2], json!({ "type": "text", "text": "describe it" }));
}

#[test]
pub(crate) fn chat_request_degrades_image_blocks_when_input_modalities_are_text_only() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "xiaomi-token-plan".to_string(),
            model: "mimo-v2.5-pro".to_string(),
        },
        messages: vec![json!({
            "role": "user",
            "content": [
                { "type": "image_url", "url": "https://example.com/image.png" },
                { "text": "describe it" }
            ],
            "timestamp_ms": 1
        })],
        tools: Vec::new(),
        metadata: json!({
            "model_metadata": {
                "capabilities": {
                    "modalities": { "input": ["text"], "output": ["text"] }
                }
            }
        }),
    };

    let body = openai_chat_request_body(&request, "https://api.xiaomimimo.com/v1");

    assert_eq!(
        body["messages"][0],
        json!({"role": "user", "content": "https://example.com/image.png\ndescribe it"})
    );
    assert!(
        !serde_json::to_string(&body)
            .expect("json")
            .contains("\"image_url\"")
    );
}

#[test]
pub(crate) fn chat_request_preserves_image_blocks_when_input_modalities_include_image() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        },
        messages: vec![json!({
            "role": "user",
            "content": [
                { "type": "image_url", "url": "https://example.com/image.png" },
                { "text": "describe it" }
            ],
            "timestamp_ms": 1
        })],
        tools: Vec::new(),
        metadata: json!({
            "model_metadata": {
                "capabilities": {
                    "modalities": { "input": ["text", "image"], "output": ["text"] }
                }
            }
        }),
    };

    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");

    assert_eq!(
        body["messages"][0]["content"][0],
        json!({
            "type": "image_url",
            "image_url": { "url": "https://example.com/image.png" }
        })
    );
}

#[test]
pub(crate) fn chat_request_degrades_image_blocks_when_attachment_is_false() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "custom".to_string(),
            model: "text-model".to_string(),
        },
        messages: vec![json!({
            "role": "user",
            "content": [
                { "type": "local_image", "path": "/tmp/screenshot.png" },
                { "type": "image_url", "url": "data:image/png;base64,aGVsbG8=" },
                { "text": "read this" }
            ],
            "timestamp_ms": 1
        })],
        tools: Vec::new(),
        metadata: json!({
            "model_metadata": {
                "capabilities": {
                    "attachment": false
                }
            }
        }),
    };

    let body = openai_chat_request_body(&request, "https://example.test/v1");

    assert_eq!(
        body["messages"][0],
        json!({
            "role": "user",
            "content": "/tmp/screenshot.png\n[image attachment omitted: data image]\nread this"
        })
    );
}

#[test]
pub(crate) fn chat_request_transcodes_bmp_local_image_to_png_part() {
    let temp = tempfile::tempdir().expect("temp");
    let image_path = temp.path().join("image.bmp");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
        .save_with_format(&image_path, image::ImageFormat::Bmp)
        .expect("bmp");
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        },
        messages: vec![json!({
            "role": "user",
            "content": [{ "type": "local_image", "path": image_path }],
            "timestamp_ms": 1
        })],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");
    let data_url = body["messages"][0]["content"][0]["image_url"]["url"]
        .as_str()
        .expect("data url");
    assert!(data_url.starts_with("data:image/png;base64,"));
}

#[test]
pub(crate) fn chat_request_resizes_large_local_image_part() {
    let temp = tempfile::tempdir().expect("temp");
    let image_path = temp.path().join("wide.png");
    image::RgbaImage::from_pixel(2501, 3, image::Rgba([0, 255, 0, 255]))
        .save_with_format(&image_path, image::ImageFormat::Png)
        .expect("png");
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        },
        messages: vec![json!({
            "role": "user",
            "content": [{ "type": "local_image", "path": image_path }],
            "timestamp_ms": 1
        })],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");
    let data_url = body["messages"][0]["content"][0]["image_url"]["url"]
        .as_str()
        .expect("data url");
    let encoded = data_url
        .strip_prefix("data:image/png;base64,")
        .expect("png data url");
    let decoded = BASE64_STANDARD.decode(encoded).expect("base64");
    let resized = image::load_from_memory(&decoded).expect("resized image");

    assert!(resized.width() <= 2000);
    assert!(resized.height() <= 2000);
}

pub(crate) fn tiny_avif_bytes() -> Vec<u8> {
    BASE64_STANDARD
        .decode(
            "AAAAIGZ0eXBhdmlmAAAAAGF2aWZtaWYxbWlhZk1BMUIAAAD5bWV0YQAAAAAAAAAvaGRscgAAAAAAAAAAcGljdAAAAAAAAAAAAAAAAFBpY3R1cmVIYW5kbGVyAAAAAA5waXRtAAAAAAABAAAAHmlsb2MAAAAARAAAAQABAAAAAQAAASEAAAAdAAAAKGlpbmYAAAAAAAEAAAAaaW5mZQIAAAAAAQAAYXYwMUNvbG9yAAAAAGppcHJwAAAAS2lwY28AAAAUaXNwZQAAAAAAAAAQAAAAEAAAABBwaXhpAAAAAAMICAgAAAAMYXYxQ4EADAAAAAATY29scm5jbHgAAgACAAIAAAAAF2lwbWEAAAAAAAAAAQABBAECgwQAAAAlbWRhdAoGGAz/2wCAMhMYAAAAUAAAAACpjmy2qrHGtoVA",
        )
        .expect("tiny avif")
}

#[test]
pub(crate) fn openai_chat_token_count_splits_context_categories() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "deepseek".to_string(),
            model: "deepseek-chat".to_string(),
        },
        messages: vec![
            json!({"role":"system","content":"mode","metadata":{"prompt_slot":"base/mode","prompt_semantic_role":"base_policy"}}),
            json!({"role":"system","content":"<available_skills>\n  <skill>\n    <name>alpha</name>\n    <description>longer helper</description>\n  </skill>\n  <skill>\n    <name>beta</name>\n    <description>short</description>\n  </skill>\n</available_skills>","metadata":{"prompt_slot":"skill_index","prompt_semantic_role":"developer_prompt"}}),
            json!({"role":"user","content":[{"text":"project instructions"}],"metadata":{"context_category":"project_context"}}),
            json!({"role":"user","content":[{"text":"previous"}]}),
            json!({"role":"user","content":[{"text":"selected skill body"}],"metadata":{"context_category":"turn_context"}}),
            json!({"role":"assistant","content":[{"type":"text","text":"ok"}]}),
        ],
        tools: vec![ToolDeclaration::new("read", "read file", json!({"type":"object"})).into()],
        metadata: json!({
            "context_counting": {
                "system_prompt_message_count": 1,
                "skill_index_message_count": 1,
                "previous_message_count": 1,
                "project_instruction_context_message_count": 1,
                "selected_skill_context_message_count": 1,
                "skill_names": ["alpha", "beta"]
            }
        }),
    };

    let count = count_openai_chat_request(&request, "https://api.deepseek.com/v1");

    assert!(count.base_policy_tokens > 0);
    assert!(count.developer_prompt_tokens > 0);
    assert!(count.system_tools_tokens > 0);
    assert_eq!(count.skills_tokens, 0);
    assert!(count.history_tokens > 0);
    assert!(count.turn_context_tokens > 0);
    assert!(count.current_prompt_tokens > 0);
    assert!(count.messages_tokens > 0);
    assert!(count.project_instruction_context_tokens > 0);
    assert!(count.selected_skill_context_tokens > 0);
    assert_eq!(count.tool_count, 1);
    assert_eq!(count.role_counts["user"].count, 3);
    assert_eq!(count.role_counts["assistant"].count, 1);
    assert_eq!(count.selected_skill_context_count, 1);
    assert_eq!(count.project_instruction_context_count, 1);
    assert_eq!(count.skill_names, vec!["alpha", "beta"]);
    assert_eq!(count.skill_entries.len(), 2);
    assert_eq!(count.skill_entries[0].name, "alpha");
    assert!(count.skill_entries[0].tokens > count.skill_entries[1].tokens);
    assert_eq!(count.encoding, "deepseek_v3");
}

#[test]
pub(crate) fn token_count_stays_equivalent_for_growing_serialized_transcripts() {
    let enc = tiktoken::get_encoding("o200k_base").expect("o200k encoding");
    let mut transcript = String::new();
    for index in 0..256 {
        transcript.push_str(&format!(
            "{{\"role\":\"tool\",\"call_id\":\"call_read_{index}\",\"content\":\"fixture content {index}\\n\"}}\n"
        ));
    }
    let expected = enc.encode(&transcript).len() as u64;

    thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..32 {
                    assert_eq!(count_text(enc, &transcript), expected);
                }
            });
        }
    });

    let unicode = format!("{transcript}中文工具结果 — 完成");
    assert_eq!(count_text(enc, &unicode), enc.encode(&unicode).len() as u64);
}

#[test]
pub(crate) fn chat_request_maps_developer_role_only_when_capability_enabled() {
    let mut request = GenerationRequest {
        model: ModelTarget {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        },
        messages: vec![json!({"role":"developer","content":"developer policy"})],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");
    assert_eq!(body["messages"][0]["role"], "system");

    request.metadata = json!({
        "model_metadata": {
            "capabilities": {
                "developer_role": false
            }
        }
    });
    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");
    assert_eq!(body["messages"][0]["role"], "system");

    request.metadata = json!({
        "model_metadata": {
            "capabilities": {
                "developer_role": true
            }
        }
    });
    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");
    assert_eq!(body["messages"][0]["role"], "developer");
}

#[test]
pub(crate) fn chat_request_preserves_user_context_message_boundaries() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        },
        messages: vec![
            json!({"role":"system","content":"mode"}),
            json!({"role":"user","content":[
                {"type":"contextual_text","text":"# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nUse repo rules.\n</INSTRUCTIONS>"},
                {"type":"contextual_text","text":"# AGENTS.md instructions for /repo/app\n\n<INSTRUCTIONS>\nUse app rules.\n</INSTRUCTIONS>"}
            ]}),
            json!({"role":"user","content":[{"text":"<skill>\n<name>reviewer</name>\nbody\n</skill>"}]}),
            json!({
                "role":"assistant",
                "content":[{"type":"reasoning","text":"private"}],
                "timestamp_ms":2,
                "finish_reason":"stop",
                "outcome":"normal",
                "model":"gpt-test",
                "provider":"openai"
            }),
            json!({"role":"user","content":[{"text":"$reviewer check this"}]}),
        ],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");
    let messages = body["messages"].as_array().expect("messages");

    assert_eq!(messages.len(), 4);
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(
        messages[1]["content"],
        "# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nUse repo rules.\n</INSTRUCTIONS>\n\n# AGENTS.md instructions for /repo/app\n\n<INSTRUCTIONS>\nUse app rules.\n</INSTRUCTIONS>"
    );
    assert_eq!(
        messages[2]["content"],
        "<skill>\n<name>reviewer</name>\nbody\n</skill>"
    );
    assert_eq!(messages[3]["content"], "$reviewer check this");
}

#[test]
pub(crate) fn chat_request_preserves_ephemeral_system_messages() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        },
        messages: vec![
            json!({"role":"system","content":"plan mode instruction"}),
            json!({"role":"user","content":[{"text":"hello"}],"timestamp_ms":1}),
        ],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");
    assert_eq!(
        body["messages"],
        json!([
            {"role":"system","content":"plan mode instruction"},
            {"role":"user","content":"hello"}
        ])
    );
}

#[test]
pub(crate) fn chat_request_hides_reasoning_unless_target_derives_protocol_echo() {
    let assistant = json!({
        "role": "assistant",
        "content": [
            { "type": "reasoning", "text": "private thought" },
            { "type": "text", "text": "visible" }
        ],
        "timestamp_ms": 2,
        "finish_reason": "stop",
        "outcome": "normal",
        "model": "m",
        "provider": "deepseek"
    });
    let mut request = GenerationRequest {
        model: ModelTarget {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        },
        messages: vec![assistant.clone()],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");
    assert_eq!(
        body["messages"][0],
        json!({"role":"assistant","content":"visible"})
    );

    request.model.provider = "deepseek".to_string();
    request.model.model = "deepseek-v4-pro".to_string();
    let body = openai_chat_request_body(&request, "https://api.deepseek.com/v1");
    assert_eq!(body["messages"][0]["content"], "visible");
    assert_eq!(body["messages"][0]["reasoning_content"], "private thought");
}

#[test]
pub(crate) fn chat_request_projects_fallback_target_reasoning_across_provider_family() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "deepseek".to_string(),
            model: "deepseek-v4-pro".to_string(),
        },
        messages: vec![json!({
            "role": "assistant",
            "content": [
                { "type": "reasoning", "text": "other provider thought" },
                {
                    "type": "tool_call",
                    "id": "call_1",
                    "name": "read",
                    "arguments": { "path": "a" },
                    "arguments_json": "{\"path\":\"a\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }
            ],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "m",
            "provider": "other"
        })],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = openai_chat_request_body(&request, "https://api.deepseek.com/v1");
    assert_eq!(
        body["messages"][0]["reasoning_content"],
        "other provider thought"
    );
}

#[test]
pub(crate) fn chat_request_projects_metadata_interleaved_reasoning_for_tool_calls() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "custom".to_string(),
            model: "custom-reasoning".to_string(),
        },
        messages: vec![json!({
            "role": "assistant",
            "content": [
                { "type": "reasoning", "text": "metadata-driven thought" },
                {
                    "type": "tool_call",
                    "id": "call_1",
                    "name": "exec_command",
                    "arguments": { "cmd": "ls" },
                    "arguments_json": "{\"cmd\":\"ls\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }
            ],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "other-model",
            "provider": "other"
        })],
        tools: Vec::new(),
        metadata: interleaved_reasoning_metadata(),
    };

    let body = openai_chat_request_body(&request, "https://example.test/v1");
    assert_eq!(
        body["messages"][0]["reasoning_content"],
        "metadata-driven thought"
    );
}

#[test]
pub(crate) fn chat_request_defaults_reasoning_content_when_reasoning_true_without_interleaved() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "custom".to_string(),
            model: "custom-reasoning".to_string(),
        },
        messages: vec![assistant_reasoning_text_message("defaulted thought")],
        tools: Vec::new(),
        metadata: reasoning_capability_metadata(),
    };

    let body = openai_chat_request_body(&request, "https://example.test/v1");
    assert_eq!(
        body["messages"][0]["reasoning_content"],
        "defaulted thought"
    );
}

#[test]
pub(crate) fn chat_request_defaults_reasoning_content_when_interleaved_true() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "custom".to_string(),
            model: "custom-reasoning".to_string(),
        },
        messages: vec![assistant_reasoning_text_message(
            "boolean interleaved thought",
        )],
        tools: Vec::new(),
        metadata: reasoning_capability_with_interleaved(json!(true)),
    };

    let body = openai_chat_request_body(&request, "https://example.test/v1");
    assert_eq!(
        body["messages"][0]["reasoning_content"],
        "boolean interleaved thought"
    );
}

#[test]
pub(crate) fn chat_request_respects_interleaved_false_even_when_reasoning_true() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "deepseek".to_string(),
            model: "deepseek-v4-pro".to_string(),
        },
        messages: vec![assistant_reasoning_text_message("disabled thought")],
        tools: Vec::new(),
        metadata: reasoning_capability_with_interleaved(json!(false)),
    };

    let body = openai_chat_request_body(&request, "https://api.deepseek.com/v1");
    assert_eq!(
        body["messages"][0],
        json!({"role":"assistant","content":"visible"})
    );
}

#[test]
pub(crate) fn chat_request_does_not_rewrite_reasoning_details_to_reasoning_content() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "custom".to_string(),
            model: "custom-reasoning".to_string(),
        },
        messages: vec![assistant_reasoning_text_message("details thought")],
        tools: Vec::new(),
        metadata: reasoning_capability_with_interleaved(json!({
            "field": "reasoning_details"
        })),
    };

    let body = openai_chat_request_body(&request, "https://example.test/v1");
    assert_eq!(
        body["messages"][0],
        json!({"role":"assistant","content":"visible"})
    );
}

#[test]
pub(crate) fn chat_request_does_not_default_reasoning_content_when_reasoning_false() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "custom".to_string(),
            model: "custom-model".to_string(),
        },
        messages: vec![assistant_reasoning_text_message(
            "disabled reasoning thought",
        )],
        tools: Vec::new(),
        metadata: json!({
            "model_metadata": {
                "capabilities": {
                    "reasoning": false
                }
            }
        }),
    };

    let body = openai_chat_request_body(&request, "https://example.test/v1");
    assert_eq!(
        body["messages"][0],
        json!({"role":"assistant","content":"visible"})
    );
}

#[test]
pub(crate) fn chat_request_replays_xiaomi_thinking_reasoning_for_tool_calls() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "xiaomi-token-plan".to_string(),
            model: "mimo-v2-omni".to_string(),
        },
        messages: vec![json!({
            "role": "assistant",
            "content": [
                { "type": "reasoning", "text": "need to inspect files" },
                {
                    "type": "tool_call",
                    "id": "call_1",
                    "name": "exec_command",
                    "arguments": { "cmd": "ls" },
                    "arguments_json": "{\"cmd\":\"ls\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }
            ],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "mimo-v2-omni",
            "provider": "xiaomi-token-plan"
        })],
        tools: Vec::new(),
        metadata: json!({
            "reasoning_effort": "low",
            "model_metadata": {
                "capabilities": {
                    "interleaved": { "field": "reasoning_content" }
                }
            }
        }),
    };

    let body = openai_chat_request_body(&request, "https://api.xiaomimimo.com/v1");
    assert_eq!(
        body["messages"][0]["reasoning_content"],
        "need to inspect files"
    );
}

#[test]
pub(crate) fn chat_request_projects_cross_provider_reasoning_for_interleaved_target() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "xiaomi-token-plan".to_string(),
            model: "mimo-v2-omni".to_string(),
        },
        messages: vec![json!({
            "role": "assistant",
            "content": [
                { "type": "reasoning", "text": "deepseek scratchpad" },
                { "type": "text", "text": "visible" }
            ],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "deepseek-v4-pro",
            "provider": "deepseek"
        })],
        tools: Vec::new(),
        metadata: json!({
            "reasoning_effort": "low",
            "model_metadata": {
                "capabilities": {
                    "interleaved": { "field": "reasoning_content" }
                }
            }
        }),
    };

    let body = openai_chat_request_body(&request, "https://api.xiaomimimo.com/v1");
    assert_eq!(
        body["messages"][0]["reasoning_content"],
        "deepseek scratchpad"
    );
}

#[test]
pub(crate) fn chat_request_pads_interleaved_reasoning_when_retained_reasoning_empty() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "custom".to_string(),
            model: "custom-reasoning".to_string(),
        },
        messages: vec![json!({
            "role": "assistant",
            "content": [
                {
                    "type": "tool_call",
                    "id": "call_1",
                    "name": "exec_command",
                    "arguments": { "cmd": "ls" },
                    "arguments_json": "{\"cmd\":\"ls\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }
            ],
            "timestamp_ms": 2,
            "finish_reason": "tool_calls",
            "outcome": "normal",
            "model": "custom-reasoning",
            "provider": "custom"
        })],
        tools: Vec::new(),
        metadata: interleaved_reasoning_metadata(),
    };

    let body = openai_chat_request_body(&request, "https://example.test/v1");
    assert_eq!(body["messages"][0]["reasoning_content"], " ");
}

#[test]
pub(crate) fn chat_request_does_not_project_reasoning_without_interleaved_or_fallback() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "custom".to_string(),
            model: "custom-model".to_string(),
        },
        messages: vec![json!({
            "role": "assistant",
            "content": [
                { "type": "reasoning", "text": "local thought" },
                { "type": "text", "text": "visible" }
            ],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "custom-model",
            "provider": "custom"
        })],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = openai_chat_request_body(&request, "https://example.test/v1");
    assert_eq!(
        body["messages"][0],
        json!({"role":"assistant","content":"visible"})
    );
}

#[test]
pub(crate) fn translate_messages_drops_thinking_only_without_merging_source_users() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        },
        messages: vec![
            json!({"role":"user","content":[{"text":"first"}],"timestamp_ms":1}),
            json!({
                "role":"assistant",
                "content":[{"type":"reasoning","text":"thinking only"}],
                "timestamp_ms":2,
                "finish_reason":"stop",
                "outcome":"normal",
                "model":"m",
                "provider":"p"
            }),
            json!({"role":"user","content":[{"text":"second"}],"timestamp_ms":3}),
        ],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");
    assert_eq!(
        body["messages"],
        json!([
            {"role":"user","content":"first"},
            {"role":"user","content":"second"}
        ])
    );
}

#[test]
pub(crate) fn translate_messages_merges_text_blocks_within_one_source_user_message() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        },
        messages: vec![json!({
            "role":"user",
            "content":[{"text":"first"},{"text":"second"}],
            "timestamp_ms":1
        })],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");
    assert_eq!(
        body["messages"],
        json!([{"role":"user","content":"first\n\nsecond"}])
    );
}

#[test]
pub(crate) fn sse_parser_handles_chunking_bom_crlf_and_done() {
    let mut parser = SseParser::new();
    let first =
        "\u{FEFF}: keepalive\r\ndata: {\"id\":\"x\",\"choices\":[{\"delta\":{\"content\":\"he";
    let second = "llo\"},\"finish_reason\":null}],\"usage\":null}\r\n\r\ndata: [DONE]\r\n\r\n";
    assert!(parser.push(first.as_bytes()).expect("first").is_empty());
    let events = parser.push(second.as_bytes()).expect("second");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].choices[0].delta.content.as_deref(), Some("hello"));
    assert!(parser.done_seen());
}

#[test]
pub(crate) fn sse_parser_handles_split_utf8_and_line_endings() {
    let mut parser = SseParser::new();
    let payload =
        "data: {\"choices\":[{\"delta\":{\"content\":\"hi 中\"},\"finish_reason\":\"stop\"}]}\r";
    let split = payload.find("中").expect("utf8");
    assert!(
        parser
            .push(&payload.as_bytes()[..split + 1])
            .expect("partial")
            .is_empty()
    );
    let mut rest = payload.as_bytes()[split + 1..].to_vec();
    rest.extend_from_slice(b"\n\rdata: [DONE]\r\n\r\n");
    let events = parser.push(&rest).expect("rest");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].choices[0].delta.content.as_deref(), Some("hi 中"));
    assert_eq!(events[0].choices[0].finish_reason.as_deref(), Some("stop"));
    assert!(parser.done_seen());
}

#[test]
pub(crate) fn sse_parser_handles_multiline_data_lf_and_comments() {
    let mut parser = SseParser::new();
    let input = concat!(
        ": ignore\n",
        "event: message\n",
        "data: {\"choices\":\n",
        "data: [{\"delta\":{\"content\":\"multi\"},\"finish_reason\":null}]}\n",
        "\n",
        "data: [DONE]\n\n"
    );
    let events = parser.push(input.as_bytes()).expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].choices[0].delta.content.as_deref(), Some("multi"));
    assert!(parser.done_seen());
}

#[test]
pub(crate) fn sse_parser_reports_provider_error_objects() {
    let mut parser = SseParser::new();
    let err = parser
        .push(b"data: {\"error\":{\"message\":\"bad key\"}}\n\n")
        .expect_err("provider error");
    assert!(err.to_string().contains("bad key"));
}

#[test]
pub(crate) fn sse_parser_rejects_premature_eof() {
    let mut parser = SseParser::new();
    let events = parser
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"stop\"}]}\n\n")
            .expect("event");
    assert_eq!(events.len(), 1);
    assert!(!parser.done_seen());
}

#[test]
pub(crate) fn chat_chunk_normalizer_streams_tool_calls() {
    let mut normalizer = ChatChunkNormalizer::new("gpt-test".to_string());
    let chunks = vec![
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":"{\"pa"}}]},"finish_reason":null}]}"#,
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a\"}"}}]},"finish_reason":"tool_calls"}]}"#,
    ];
    let mut events = Vec::new();
    for chunk in chunks {
        let chunk = serde_json::from_str::<ChatCompletionChunk>(chunk).expect("chunk");
        events.extend(normalizer.ingest(chunk).expect("ingest"));
    }
    events.extend(normalizer.finish());

    assert!(events.iter().any(|event| {
        matches!(
            event,
            StreamEvent::ToolCallStart {
                id,
                name,
                ..
            } if id == "call_1" && name == "read"
        )
    }));
    assert!(
        events
            .iter()
            .any(|event| { matches!(event, StreamEvent::ToolCallEnd { call_index: 0, .. }) })
    );
    assert_eq!(
        events.last(),
        Some(&StreamEvent::Done {
            outcome: Outcome::Normal,
            finish_reason: Some("tool_calls".to_string())
        })
    );
}

#[test]
pub(crate) fn chat_chunk_normalizer_handles_null_tool_calls_and_usage() {
    let mut normalizer = ChatChunkNormalizer::new("gpt-test".to_string());
    let chunk = serde_json::from_str::<ChatCompletionChunk>(
            r#"{"id":"resp_1","model":"gpt-test","choices":[{"delta":{"content":"ok","tool_calls":null},"finish_reason":"stop"}],"usage":{"total_tokens":7}}"#,
        )
        .expect("chunk");
    let mut events = normalizer.ingest(chunk).expect("ingest");
    events.extend(normalizer.finish());

    assert!(events.iter().any(|event| {
        matches!(
            event,
            StreamEvent::Usage { usage } if usage["total_tokens"] == 7
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            StreamEvent::Metadata { metadata } if metadata["provider_response_id"] == "resp_1"
        )
    }));
    assert!(
        events
            .iter()
            .any(|event| { matches!(event, StreamEvent::TextDelta { text } if text == "ok") })
    );
    assert_eq!(
        events.last(),
        Some(&StreamEvent::Done {
            outcome: Outcome::Normal,
            finish_reason: Some("stop".to_string())
        })
    );
}

#[test]
pub(crate) fn normalizes_usage_to_provider_neutral_fields() {
    let usage = json!({
        "prompt_tokens": 11,
        "completion_tokens": 7,
        "total_tokens": 18,
        "completion_tokens_details": { "reasoning_tokens": 3 },
        "prompt_tokens_details": { "cached_tokens": 5 },
        "ignored": "x"
    });
    let normalized = normalize_usage(&usage).expect("usage");
    assert_eq!(normalized["input_tokens"], 11);
    assert_eq!(normalized["output_tokens"], 7);
    assert_eq!(normalized["total_tokens"], 18);
    assert_eq!(normalized["reasoning_tokens"], 3);
    assert_eq!(normalized["cached_tokens"], 5);
    assert!(normalized.get("ignored").is_none());
}

#[test]
pub(crate) fn allowlists_provider_metadata() {
    let metadata = json!({
        "id": "resp_1",
        "model": "gpt-test",
        "system_fingerprint": "fp",
        "headers": { "authorization": "secret" },
        "raw": ["not", "allowed"]
    });
    let normalized = allowlisted_provider_metadata(&metadata).expect("metadata");
    assert_eq!(normalized["provider_response_id"], "resp_1");
    assert_eq!(normalized["model"], "gpt-test");
    assert_eq!(normalized["system_fingerprint"], "fp");
    assert!(normalized.get("headers").is_none());
    assert!(normalized.get("raw").is_none());
}

#[test]
pub(crate) fn chat_chunk_normalizer_streams_reasoning_fields() {
    let mut normalizer = ChatChunkNormalizer::new("gpt-test".to_string());
    let chunk = serde_json::from_str::<ChatCompletionChunk>(
            r#"{"choices":[{"delta":{"reasoning_content":"deep thought","reasoning_details":[{"type":"thinking","text":"detail"}],"content":"answer"},"finish_reason":"stop"}]}"#,
        )
        .expect("chunk");
    let events = normalizer.ingest(chunk).expect("ingest");

    assert!(events.iter().any(|event| {
        matches!(
            event,
            StreamEvent::ReasoningDelta { text, reasoning_content: Some(provider_text) }
                if text == "deep thought" && provider_text == "deep thought"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            StreamEvent::ReasoningDetails { details }
                if details[0]["type"] == "thinking"
        )
    }));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::TextDelta { text } if text == "answer"))
    );
}
