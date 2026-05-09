use super::*;
use futures::stream;
use psychevo_ai::{FakeProvider, RawStreamEvent};
use std::sync::Mutex;

#[derive(Default)]
struct CaptureSink {
    events: Mutex<Vec<AgentEvent>>,
}

impl EventSink for CaptureSink {
    fn emit(&self, event: AgentEvent) -> BoxFuture<'static, Result<()>> {
        self.events.lock().expect("events").push(event);
        Box::pin(async { Ok(()) })
    }
}

#[derive(Clone)]
struct StaticProvider {
    events: Vec<StreamEvent>,
}

impl GenerationProvider for StaticProvider {
    fn stream(
        &self,
        _request: GenerationRequest,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, psychevo_ai::Result<psychevo_ai::GenerationStream>> {
        let events = self.events.clone().into_iter().map(Ok);
        Box::pin(async move {
            let output: psychevo_ai::GenerationStream = Box::pin(stream::iter(events));
            Ok(output)
        })
    }
}

fn request() -> AgentLoopRequest {
    AgentLoopRequest {
        model_provider: "fake".to_string(),
        model: "model".to_string(),
        generation_metadata: json!({}),
        system_instructions: Vec::new(),
        previous_messages: Vec::new(),
        context_messages: Vec::new(),
        prompt_messages: vec![user_text_message("hello")],
        tools: Vec::new(),
        max_turns: 1,
    }
}

#[tokio::test]
async fn reasoning_only_progress_has_no_visible_message_update() {
    let provider = Arc::new(FakeProvider::new(vec![vec![
        RawStreamEvent::Reasoning("private".to_string()),
        RawStreamEvent::Text("visible".to_string()),
        RawStreamEvent::Done(Outcome::Normal),
    ]]));
    let sink = Arc::new(CaptureSink::default());
    let (_, control) = ControlHandle::new();
    let completion = run_agent_loop(provider, request(), sink.clone(), control)
        .await
        .expect("loop");
    assert_eq!(completion.outcome, Outcome::Normal);

    let events = sink.events.lock().expect("events");
    assert!(events.iter().any(|event| {
        matches!(event, AgentEvent::ReasoningDelta { text } if text == "private")
    }));
    let updates = events
        .iter()
        .filter(|event| matches!(event, AgentEvent::MessageUpdate { .. }))
        .count();
    assert_eq!(updates, 1);
}

#[tokio::test]
async fn usage_and_metadata_do_not_emit_empty_message_updates() {
    let provider = Arc::new(StaticProvider {
        events: vec![
            StreamEvent::Metadata {
                metadata: json!({"id":"resp"}),
            },
            StreamEvent::Usage {
                usage: json!({"total_tokens":1}),
            },
            StreamEvent::TextDelta {
                text: "ok".to_string(),
            },
            StreamEvent::Done {
                outcome: Outcome::Normal,
                finish_reason: Some("stop".to_string()),
            },
        ],
    });
    let sink = Arc::new(CaptureSink::default());
    let (_, control) = ControlHandle::new();
    run_agent_loop(provider, request(), sink.clone(), control)
        .await
        .expect("loop");

    let events = sink.events.lock().expect("events");
    let updates = events
        .iter()
        .filter(|event| matches!(event, AgentEvent::MessageUpdate { .. }))
        .count();
    assert_eq!(updates, 1);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            AgentEvent::MessageEnd { usage: Some(usage), metadata: Some(metadata), .. }
                if usage["total_tokens"] == 1
                    && metadata["provider_response_id"] == "resp"
        )
    }));
}

#[tokio::test]
async fn complete_inline_think_blocks_are_folded_reasoning() {
    let provider = Arc::new(FakeProvider::new(vec![vec![
        RawStreamEvent::Text("visible <think>secret</think> done".to_string()),
        RawStreamEvent::Done(Outcome::Normal),
    ]]));
    let sink = Arc::new(CaptureSink::default());
    let (_, control) = ControlHandle::new();
    let completion = run_agent_loop(provider, request(), sink.clone(), control)
        .await
        .expect("loop");
    let assistant = completion
        .messages
        .iter()
        .find(|message| matches!(message, Message::Assistant { .. }))
        .expect("assistant");
    let Message::Assistant { content, .. } = assistant else {
        unreachable!();
    };
    assert!(content.contains(&AssistantBlock::Reasoning {
        text: "secret".to_string(),
        provider_evidence: None,
    }));
    assert!(content.contains(&AssistantBlock::Text {
        text: "visible  done".to_string()
    }));

    let events = sink.events.lock().expect("events");
    assert!(
        events.iter().any(|event| {
            matches!(event, AgentEvent::ReasoningEnd { text } if text == "secret")
        })
    );
}

#[tokio::test]
async fn reasoning_details_attach_to_reasoning_block_evidence() {
    let provider = Arc::new(StaticProvider {
        events: vec![
            StreamEvent::ReasoningDelta {
                text: "scratch".to_string(),
                reasoning_content: Some("scratch".to_string()),
            },
            StreamEvent::ReasoningDetails {
                details: json!([{ "type": "thinking", "text": "opaque" }]),
            },
            StreamEvent::TextDelta {
                text: "visible".to_string(),
            },
            StreamEvent::Done {
                outcome: Outcome::Normal,
                finish_reason: Some("stop".to_string()),
            },
        ],
    });
    let sink = Arc::new(CaptureSink::default());
    let (_, control) = ControlHandle::new();
    let completion = run_agent_loop(provider, request(), sink, control)
        .await
        .expect("loop");
    let assistant = completion
        .messages
        .iter()
        .find(|message| matches!(message, Message::Assistant { .. }))
        .expect("assistant");
    let Message::Assistant { content, .. } = assistant else {
        unreachable!();
    };
    let reasoning = content
        .iter()
        .find_map(|block| match block {
            AssistantBlock::Reasoning {
                text,
                provider_evidence,
            } => Some((text, provider_evidence)),
            _ => None,
        })
        .expect("reasoning block");
    assert_eq!(reasoning.0, "scratch");
    assert_eq!(
        reasoning.1.as_ref().expect("evidence")["reasoning_details"][0]["type"],
        "thinking"
    );
}
