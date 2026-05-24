use futures::StreamExt;
use psychevo_ai::{
    AbortSignal, FakeProvider, GenerationProvider, GenerationRequest, ModelTarget, Outcome,
    RawStreamEvent, StreamEvent,
};
use serde_json::json;
use tokio::sync::watch;

pub(crate) fn request() -> GenerationRequest {
    GenerationRequest {
        model: ModelTarget {
            provider: "fake".to_string(),
            model: "fake".to_string(),
        },
        messages: vec![],
        tools: vec![],
        metadata: json!({}),
    }
}

#[tokio::test]
pub(crate) async fn fake_provider_normalizes_raw_stream_events() {
    let provider = FakeProvider::new(vec![vec![
        RawStreamEvent::Text("hello".to_string()),
        RawStreamEvent::ToolStart {
            content_index: 0,
            call_index: 0,
            id: "call-1".to_string(),
            name: "read".to_string(),
        },
        RawStreamEvent::ToolArgs {
            content_index: 0,
            call_index: 0,
            delta: "{\"path\":\"x\"}".to_string(),
        },
        RawStreamEvent::ToolEnd {
            content_index: 0,
            call_index: 0,
        },
        RawStreamEvent::Done(Outcome::Normal),
    ]]);
    let (_tx, rx) = watch::channel(false);
    let mut stream = provider
        .stream(request(), AbortSignal::new(rx))
        .await
        .expect("stream");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("event"));
    }
    assert_eq!(
        events,
        vec![
            StreamEvent::TextDelta {
                text: "hello".to_string()
            },
            StreamEvent::ToolCallStart {
                content_index: 0,
                call_index: 0,
                id: "call-1".to_string(),
                name: "read".to_string()
            },
            StreamEvent::ToolCallDelta {
                content_index: 0,
                call_index: 0,
                id: None,
                name: None,
                arguments_delta: "{\"path\":\"x\"}".to_string()
            },
            StreamEvent::ToolCallEnd {
                content_index: 0,
                call_index: 0
            },
            StreamEvent::Done {
                outcome: Outcome::Normal,
                finish_reason: None
            }
        ]
    );
}

#[tokio::test]
pub(crate) async fn fake_provider_observes_abort_before_generation() {
    let provider = FakeProvider::new(vec![vec![RawStreamEvent::Text("unused".to_string())]]);
    let (tx, rx) = watch::channel(false);
    tx.send(true).expect("abort");
    let mut stream = provider
        .stream(request(), AbortSignal::new(rx))
        .await
        .expect("stream");
    let event = stream.next().await.expect("one event").expect("event");
    assert_eq!(
        event,
        StreamEvent::Done {
            outcome: Outcome::Aborted,
            finish_reason: Some("aborted".to_string())
        }
    );
}
