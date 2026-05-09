use super::*;
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

    let body = build_chat_request(&request, "https://api.openai.com/v1");
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

    let body = build_chat_request(&request, "https://api.openai.com/v1");
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

    let body = build_chat_request(&request, "https://api.openai.com/v1");
    assert_eq!(
        body["messages"][0],
        json!({"role":"assistant","content":"visible"})
    );

    request.model.provider = "deepseek".to_string();
    request.model.model = "deepseek-v4-pro".to_string();
    let body = build_chat_request(&request, "https://api.deepseek.com/v1");
    assert_eq!(body["messages"][0]["content"], "visible");
    assert_eq!(body["messages"][0]["reasoning_content"], "private thought");
}

#[test]
fn chat_request_pads_target_reasoning_without_cross_provider_leak() {
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

    let body = build_chat_request(&request, "https://api.deepseek.com/v1");
    assert_eq!(body["messages"][0]["reasoning_content"], " ");
    assert!(
        !serde_json::to_string(&body)
            .expect("body")
            .contains("other provider thought")
    );
}

#[test]
fn chat_request_does_not_replay_cross_provider_reasoning_text() {
    let request = GenerationRequest {
        model: ModelTarget {
            provider: "deepseek".to_string(),
            model: "deepseek-v4-pro".to_string(),
        },
        messages: vec![json!({
            "role": "assistant",
            "content": [
                { "type": "reasoning", "text": "xiaomi scratchpad" },
                { "type": "text", "text": "visible" }
            ],
            "timestamp_ms": 2,
            "finish_reason": "stop",
            "outcome": "normal",
            "model": "mimo",
            "provider": "xiaomi"
        })],
        tools: Vec::new(),
        metadata: json!({}),
    };

    let body = build_chat_request(&request, "https://api.deepseek.com/v1");
    assert_eq!(body["messages"][0]["reasoning_content"], " ");
    assert!(
        !serde_json::to_string(&body)
            .expect("body")
            .contains("xiaomi scratchpad")
    );
}

#[test]
fn translate_messages_drops_thinking_only_and_merges_users() {
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

    let body = build_chat_request(&request, "https://api.openai.com/v1");
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
