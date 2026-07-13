#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn basic_generation_request() -> GenerationRequest {
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

pub(crate) fn interleaved_reasoning_metadata() -> serde_json::Value {
    json!({
        "model_metadata": {
            "capabilities": {
                "interleaved": { "field": "reasoning_content" }
            }
        }
    })
}

pub(crate) fn reasoning_capability_metadata() -> serde_json::Value {
    json!({
        "model_metadata": {
            "capabilities": {
                "reasoning": true
            }
        }
    })
}

pub(crate) fn reasoning_capability_with_interleaved(
    interleaved: serde_json::Value,
) -> serde_json::Value {
    json!({
        "model_metadata": {
            "capabilities": {
                "reasoning": true,
                "interleaved": interleaved
            }
        }
    })
}

pub(crate) fn assistant_reasoning_text_message(reasoning: &str) -> serde_json::Value {
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

pub(crate) fn read_http_headers(stream: &mut std::net::TcpStream) {
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

pub(crate) fn read_http_request(stream: &mut std::net::TcpStream) -> String {
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

pub(crate) fn http_response(status: &str, content_type: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

pub(crate) fn http_request_json_body(request: &str) -> Value {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("http body");
    serde_json::from_str(body).expect("request json")
}

#[tokio::test]
pub(crate) async fn openai_provider_posts_public_request_body() {
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
pub(crate) async fn openai_provider_retries_image_rejection_as_text() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let base_url = format!("http://{addr}/v1");
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("first accept");
        let request = read_http_request(&mut stream);
        request_tx.send(request).expect("first request");
        let body = r#"{"error":{"message":"No endpoints found that support image input"}}"#;
        stream
            .write_all(&http_response("404 Not Found", "application/json", body))
            .expect("first response");

        let (mut stream, _) = listener.accept().expect("second accept");
        let request = read_http_request(&mut stream);
        request_tx.send(request).expect("second request");
        stream
            .write_all(&http_response(
                "200 OK",
                "text/event-stream",
                "data: [DONE]\n\n",
            ))
            .expect("second response");
    });

    let request = GenerationRequest {
        model: ModelTarget {
            provider: "xiaomi-token-plan".to_string(),
            model: "mimo-v2.5-pro".to_string(),
        },
        messages: vec![json!({
            "role": "user",
            "content": [
                { "type": "image_url", "url": "https://developers.openai.com/codex/hooks" },
                { "text": "summarize this page" }
            ],
            "timestamp_ms": 1
        })],
        tools: Vec::new(),
        metadata: json!({}),
    };
    let provider = OpenAiChatProvider::new(base_url, "test-key", "xiaomi-token-plan");
    let (_abort_tx, abort_rx) = watch::channel(false);
    let _stream = provider
        .stream(request, AbortSignal::new(abort_rx))
        .await
        .expect("stream");

    let first = request_rx.recv().expect("first captured request");
    let second = request_rx.recv().expect("second captured request");
    let first_body = http_request_json_body(&first);
    let second_body = http_request_json_body(&second);
    assert!(
        serde_json::to_string(&first_body)
            .expect("first json")
            .contains("\"image_url\"")
    );
    let second_body_text = serde_json::to_string(&second_body).expect("second json");
    assert!(!second_body_text.contains("\"image_url\""));
    assert_eq!(
        second_body["messages"][0],
        json!({
            "role": "user",
            "content": "https://developers.openai.com/codex/hooks\nsummarize this page"
        })
    );
    server.join().expect("server");
}

#[tokio::test]
pub(crate) async fn openai_provider_omits_authorization_for_empty_api_key() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
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

    let provider = OpenAiChatProvider::new(format!("http://{addr}/v1"), "", "mock");
    let (_abort_tx, abort_rx) = watch::channel(false);
    let _stream = provider
        .stream(basic_generation_request(), AbortSignal::new(abort_rx))
        .await
        .expect("stream");
    let request = request_rx.recv().expect("captured request");
    assert!(!request.to_lowercase().contains("authorization:"));
    server.join().expect("server");
}

#[tokio::test]
pub(crate) async fn openai_provider_abort_wakes_pending_http_response() {
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
pub(crate) async fn openai_provider_abort_wakes_pending_sse_chunk() {
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
pub(crate) fn chat_request_maps_messages_and_tools() {
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
        tools: vec![ToolDeclaration::new("read", "read file", json!({ "type": "object" })).into()],
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
