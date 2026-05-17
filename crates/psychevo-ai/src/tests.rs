use super::*;
use base64::Engine as _;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

fn basic_generation_request() -> GenerationRequest {
    GenerationRequest {
        model: ModelTarget {
            provider: "mock".to_string(),
            model: "mock-model".to_string(),
        },
        messages: vec![json!({"role":"user","content":"hello"})],
        tools: Vec::new(),
        metadata: json!({}),
    }
}

fn interleaved_reasoning_metadata() -> serde_json::Value {
    json!({
        "model_metadata": {
            "capabilities": {
                "interleaved": { "field": "reasoning_content" }
            }
        }
    })
}

fn reasoning_capability_metadata() -> serde_json::Value {
    json!({
        "model_metadata": {
            "capabilities": {
                "reasoning": true
            }
        }
    })
}

fn reasoning_capability_with_interleaved(interleaved: serde_json::Value) -> serde_json::Value {
    json!({
        "model_metadata": {
            "capabilities": {
                "reasoning": true,
                "interleaved": interleaved
            }
        }
    })
}

fn assistant_reasoning_text_message(reasoning: &str) -> serde_json::Value {
    json!({
        "role": "assistant",
        "content": [
            { "type": "reasoning", "text": reasoning },
            { "type": "text", "text": "visible" }
        ],
        "timestamp_ms": 2,
        "finish_reason": "stop",
        "outcome": "normal",
        "model": "source-model",
        "provider": "source-provider"
    })
}

fn read_http_headers(stream: &mut std::net::TcpStream) {
    let mut request = Vec::new();
    let mut buf = [0; 1024];
    loop {
        let n = stream.read(&mut buf).expect("read request");
        if n == 0 {
            break;
        }
        request.extend_from_slice(&buf[..n]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
}

fn read_http_request(stream: &mut std::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut buf = [0; 1024];
    let header_end = loop {
        let n = stream.read(&mut buf).expect("read request");
        if n == 0 {
            break request.len();
        }
        request.extend_from_slice(&buf[..n]);
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while request.len().saturating_sub(header_end) < content_length {
        let n = stream.read(&mut buf).expect("read body");
        if n == 0 {
            break;
        }
        request.extend_from_slice(&buf[..n]);
    }
    String::from_utf8_lossy(&request).to_string()
}

#[tokio::test]
async fn openai_provider_posts_public_request_body() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let base_url = format!("http://{addr}/v1");
    let expected = openai_chat_request_body(&basic_generation_request(), &base_url);
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let request = read_http_request(&mut stream);
        request_tx.send(request).expect("request");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: 14\r\nconnection: close\r\n\r\ndata: [DONE]\n\n",
            )
            .expect("response");
    });

    let provider = OpenAiChatProvider::new(base_url, "test-key", "mock");
    let (_abort_tx, abort_rx) = watch::channel(false);
    let _stream = provider
        .stream(basic_generation_request(), AbortSignal::new(abort_rx))
        .await
        .expect("stream");
    let request = request_rx.recv().expect("captured request");
    assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("http body");
    let body: Value = serde_json::from_str(body).expect("request json");
    assert_eq!(body, expected);
    server.join().expect("server");
}

#[tokio::test]
async fn openai_provider_abort_wakes_pending_http_response() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        read_http_headers(&mut stream);
        accepted_tx.send(()).expect("accepted");
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buf = [0; 16];
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
    });
    let provider = OpenAiChatProvider::new(format!("http://{addr}/v1"), "test-key", "mock");
    let (abort_tx, abort_rx) = watch::channel(false);
    let task = tokio::spawn(async move {
        provider
            .stream(basic_generation_request(), AbortSignal::new(abort_rx))
            .await
    });

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if accepted_rx.try_recv().is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("server accepted request");
    abort_tx.send(true).expect("abort");
    let mut stream = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("provider stream returned after abort")
        .expect("join")
        .expect("stream");
    let event = stream.next().await.expect("aborted event").expect("event");
    assert!(matches!(
        event,
        StreamEvent::Done {
            outcome: Outcome::Aborted,
            ..
        }
    ));
    server.join().expect("server thread");
}

#[tokio::test]
async fn openai_provider_abort_wakes_pending_sse_chunk() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        read_http_headers(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n")
            .expect("write headers");
        stream.flush().expect("flush headers");
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buf = [0; 16];
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
    });
    let provider = OpenAiChatProvider::new(format!("http://{addr}/v1"), "test-key", "mock");
    let (abort_tx, abort_rx) = watch::channel(false);
    let mut stream = provider
        .stream(basic_generation_request(), AbortSignal::new(abort_rx))
        .await
        .expect("stream");

    abort_tx.send(true).expect("abort");
    let event = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("stream woke after abort")
        .expect("aborted event")
        .expect("event");
    assert!(matches!(
        event,
        StreamEvent::Done {
            outcome: Outcome::Aborted,
            ..
        }
    ));
    server.join().expect("server thread");
}

#[test]
fn chat_request_maps_messages_and_tools() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
        },
        messages: vec![
            json!({
                "role": "user",
                "content": [{ "text": "hello" }],
                "timestamp_ms": 1
            }),
            json!({
                "role": "assistant",
                "content": [{
                    "type": "tool_call",
                    "id": "call_1",
                    "name": "read",
                    "arguments": { "path": "a" },
                    "arguments_json": "{\"path\":\"a\"}",
                    "arguments_error": null,
                    "content_index": 0,
                    "call_index": 0
                }],
                "timestamp_ms": 2,
                "finish_reason": "tool_calls",
                "outcome": "normal",
                "model": "gpt-test",
                "provider": "openai"
            }),
            json!({
                "role": "tool_result",
                "tool_call_id": "call_1",
                "tool_name": "read",
                "content": "{\"ok\":true}",
                "is_error": false,
                "timestamp_ms": 3
            }),
        ],
        tools: vec![ToolDeclaration {
            name: "read".to_string(),
            description: "read file".to_string(),
            parameters: json!({ "type": "object" }),
        }],
        metadata: json!({ "reasoning_effort": "medium" }),
    };

    let body = openai_chat_request_body(&request, "https://api.openai.com/v1");
    assert_eq!(body["model"], "gpt-test");
    assert_eq!(body["stream"], true);
    assert_eq!(
        body["messages"][0],
        json!({"role": "user", "content": "hello"})
    );
    assert_eq!(
        body["messages"][1]["tool_calls"][0]["function"]["arguments"],
        "{\"path\":\"a\"}"
    );
    assert_eq!(body["messages"][2]["role"], "tool");
    assert_eq!(body["tools"][0]["function"]["name"], "read");
    assert_eq!(body["reasoning_effort"], "medium");
}

#[test]
fn chat_request_maps_local_image_blocks_to_content_parts() {
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
fn chat_request_transcodes_bmp_local_image_to_png_part() {
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
fn chat_request_resizes_large_local_image_part() {
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

fn tiny_avif_bytes() -> Vec<u8> {
    BASE64_STANDARD
        .decode(
            "AAAAIGZ0eXBhdmlmAAAAAGF2aWZtaWYxbWlhZk1BMUIAAAD5bWV0YQAAAAAAAAAvaGRscgAAAAAAAAAAcGljdAAAAAAAAAAAAAAAAFBpY3R1cmVIYW5kbGVyAAAAAA5waXRtAAAAAAABAAAAHmlsb2MAAAAARAAAAQABAAAAAQAAASEAAAAdAAAAKGlpbmYAAAAAAAEAAAAaaW5mZQIAAAAAAQAAYXYwMUNvbG9yAAAAAGppcHJwAAAAS2lwY28AAAAUaXNwZQAAAAAAAAAQAAAAEAAAABBwaXhpAAAAAAMICAgAAAAMYXYxQ4EADAAAAAATY29scm5jbHgAAgACAAIAAAAAF2lwbWEAAAAAAAAAAQABBAECgwQAAAAlbWRhdAoGGAz/2wCAMhMYAAAAUAAAAACpjmy2qrHGtoVA",
        )
        .expect("tiny avif")
}

#[test]
fn openai_chat_token_count_splits_context_categories() {
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
        tools: vec![ToolDeclaration {
            name: "read".to_string(),
            description: "read file".to_string(),
            parameters: json!({"type":"object"}),
        }],
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
fn chat_request_maps_developer_role_only_when_capability_enabled() {
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
fn chat_request_preserves_user_context_message_boundaries() {
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
fn chat_request_preserves_ephemeral_system_messages() {
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
fn chat_request_hides_reasoning_unless_target_derives_protocol_echo() {
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
fn chat_request_projects_fallback_target_reasoning_across_provider_family() {
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
fn chat_request_projects_metadata_interleaved_reasoning_for_tool_calls() {
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
                    "name": "bash",
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
fn chat_request_defaults_reasoning_content_when_reasoning_true_without_interleaved() {
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
fn chat_request_defaults_reasoning_content_when_interleaved_true() {
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
fn chat_request_respects_interleaved_false_even_when_reasoning_true() {
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
fn chat_request_does_not_rewrite_reasoning_details_to_reasoning_content() {
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
fn chat_request_does_not_default_reasoning_content_when_reasoning_false() {
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
fn chat_request_replays_xiaomi_thinking_reasoning_for_tool_calls() {
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
                    "name": "bash",
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
fn chat_request_projects_cross_provider_reasoning_for_interleaved_target() {
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
fn chat_request_pads_interleaved_reasoning_when_retained_reasoning_empty() {
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
                    "name": "bash",
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
fn chat_request_does_not_project_reasoning_without_interleaved_or_fallback() {
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
fn translate_messages_drops_thinking_only_without_merging_source_users() {
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
fn translate_messages_merges_text_blocks_within_one_source_user_message() {
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
fn sse_parser_handles_chunking_bom_crlf_and_done() {
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
fn sse_parser_handles_split_utf8_and_line_endings() {
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
fn sse_parser_handles_multiline_data_lf_and_comments() {
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
fn sse_parser_reports_provider_error_objects() {
    let mut parser = SseParser::new();
    let err = parser
        .push(b"data: {\"error\":{\"message\":\"bad key\"}}\n\n")
        .expect_err("provider error");
    assert!(err.to_string().contains("bad key"));
}

#[test]
fn sse_parser_rejects_premature_eof() {
    let mut parser = SseParser::new();
    let events = parser
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"stop\"}]}\n\n")
            .expect("event");
    assert_eq!(events.len(), 1);
    assert!(!parser.done_seen());
}

#[test]
fn chat_chunk_normalizer_streams_tool_calls() {
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
fn chat_chunk_normalizer_handles_null_tool_calls_and_usage() {
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
fn normalizes_usage_to_provider_neutral_fields() {
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
fn allowlists_provider_metadata() {
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
fn chat_chunk_normalizer_streams_reasoning_fields() {
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
