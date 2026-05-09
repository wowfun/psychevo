use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use psychevo_agent_core::{
    AgentEvent, AgentLoopRequest, ControlHandle, EventSink, Message, Result, ToolBinding,
    ToolExecutionMode, ToolOutput, run_agent_loop, user_text_message,
};
use psychevo_ai::{AbortSignal, FakeProvider, Outcome, RawStreamEvent};
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct RecordingSink {
    events: Arc<Mutex<Vec<AgentEvent>>>,
    fail_on_message_end: bool,
}

impl EventSink for RecordingSink {
    fn emit(&self, event: AgentEvent) -> BoxFuture<'static, Result<()>> {
        let events = Arc::clone(&self.events);
        let fail = self.fail_on_message_end && matches!(event, AgentEvent::MessageEnd { .. });
        Box::pin(async move {
            if fail {
                return Err(psychevo_agent_core::Error::EventSink("boom".to_string()));
            }
            events.lock().expect("events").push(event);
            Ok(())
        })
    }
}

struct DelayTool;

impl ToolBinding for DelayTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "test delay"
    }

    fn parameters(&self) -> Value {
        json!({})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        Box::pin(async move {
            let delay = args.get("delay").and_then(Value::as_u64).unwrap_or(0);
            tokio::time::sleep(Duration::from_millis(delay)).await;
            ToolOutput::ok(json!({ "delay": delay, "error": null }))
        })
    }
}

struct SequentialDelayTool;

impl ToolBinding for SequentialDelayTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "test sequential delay"
    }

    fn parameters(&self) -> Value {
        json!({})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        DelayTool.execute(tool_call_id, args, abort)
    }
}

fn tool_script() -> Vec<Vec<RawStreamEvent>> {
    vec![
        vec![
            RawStreamEvent::ToolStart {
                content_index: 0,
                call_index: 0,
                id: "slow".to_string(),
                name: "read".to_string(),
            },
            RawStreamEvent::ToolArgs {
                content_index: 0,
                call_index: 0,
                delta: "{\"delay\":50}".to_string(),
            },
            RawStreamEvent::ToolEnd {
                content_index: 0,
                call_index: 0,
            },
            RawStreamEvent::ToolStart {
                content_index: 1,
                call_index: 1,
                id: "fast".to_string(),
                name: "read".to_string(),
            },
            RawStreamEvent::ToolArgs {
                content_index: 1,
                call_index: 1,
                delta: "{\"delay\":1}".to_string(),
            },
            RawStreamEvent::ToolEnd {
                content_index: 1,
                call_index: 1,
            },
            RawStreamEvent::Done(Outcome::Normal),
        ],
        vec![
            RawStreamEvent::Text("done".to_string()),
            RawStreamEvent::Done(Outcome::Normal),
        ],
    ]
}

#[tokio::test]
async fn tool_execution_events_include_timing_fields() {
    let provider = Arc::new(FakeProvider::new(vec![
        vec![
            RawStreamEvent::ToolStart {
                content_index: 0,
                call_index: 0,
                id: "timed".to_string(),
                name: "read".to_string(),
            },
            RawStreamEvent::ToolArgs {
                content_index: 0,
                call_index: 0,
                delta: "{\"delay\":5}".to_string(),
            },
            RawStreamEvent::ToolEnd {
                content_index: 0,
                call_index: 0,
            },
            RawStreamEvent::Done(Outcome::Normal),
        ],
        vec![
            RawStreamEvent::Text("done".to_string()),
            RawStreamEvent::Done(Outcome::Normal),
        ],
    ]));
    let sink = RecordingSink::default();
    let events = Arc::clone(&sink.events);
    let (_control, receivers) = ControlHandle::new();
    run_agent_loop(
        provider,
        AgentLoopRequest {
            model_provider: "fake".to_string(),
            model: "fake".to_string(),
            generation_metadata: json!({}),
            system_instructions: Vec::new(),
            previous_messages: vec![],
            context_messages: Vec::new(),
            prompt_messages: vec![user_text_message("run")],
            tools: vec![Arc::new(DelayTool)],
            max_turns: 4,
        },
        Arc::new(sink),
        receivers,
    )
    .await
    .expect("loop");

    let guard = events.lock().expect("events");
    let started_at_ms = guard
        .iter()
        .find_map(|event| match event {
            AgentEvent::ToolExecutionStart {
                tool_call_id,
                started_at_ms,
                ..
            } if tool_call_id == "timed" => Some(*started_at_ms),
            _ => None,
        })
        .expect("tool start");
    let elapsed_ms = guard
        .iter()
        .find_map(|event| match event {
            AgentEvent::ToolExecutionEnd {
                tool_call_id,
                elapsed_ms,
                ..
            } if tool_call_id == "timed" => Some(*elapsed_ms),
            _ => None,
        })
        .expect("tool end");
    assert!(started_at_ms > 0);
    assert!(elapsed_ms > 0);
}

#[tokio::test]
async fn sequential_tool_elapsed_excludes_queue_time() {
    let provider = Arc::new(FakeProvider::new(tool_script()));
    let sink = RecordingSink::default();
    let events = Arc::clone(&sink.events);
    let (_control, receivers) = ControlHandle::new();
    run_agent_loop(
        provider,
        AgentLoopRequest {
            model_provider: "fake".to_string(),
            model: "fake".to_string(),
            generation_metadata: json!({}),
            system_instructions: Vec::new(),
            previous_messages: vec![],
            context_messages: Vec::new(),
            prompt_messages: vec![user_text_message("run")],
            tools: vec![Arc::new(SequentialDelayTool)],
            max_turns: 4,
        },
        Arc::new(sink),
        receivers,
    )
    .await
    .expect("loop");

    let guard = events.lock().expect("events");
    let elapsed_for = |id: &str| {
        guard
            .iter()
            .find_map(|event| match event {
                AgentEvent::ToolExecutionEnd {
                    tool_call_id,
                    elapsed_ms,
                    ..
                } if tool_call_id == id => Some(*elapsed_ms),
                _ => None,
            })
            .expect("tool end")
    };
    let slow_elapsed = elapsed_for("slow");
    let fast_elapsed = elapsed_for("fast");
    assert!(fast_elapsed < slow_elapsed);
}

#[tokio::test]
async fn parallel_tool_end_can_finish_before_source_ordered_tool_results() {
    let provider = Arc::new(FakeProvider::new(tool_script()));
    let sink = RecordingSink::default();
    let events = Arc::clone(&sink.events);
    let (_control, receivers) = ControlHandle::new();
    let completion = run_agent_loop(
        provider,
        AgentLoopRequest {
            model_provider: "fake".to_string(),
            model: "fake".to_string(),
            generation_metadata: json!({}),
            system_instructions: Vec::new(),
            previous_messages: vec![],
            context_messages: Vec::new(),
            prompt_messages: vec![user_text_message("run")],
            tools: vec![Arc::new(DelayTool)],
            max_turns: 4,
        },
        Arc::new(sink),
        receivers,
    )
    .await
    .expect("loop");
    assert_eq!(completion.outcome, Outcome::Normal);

    let tool_result_ids = completion
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult { tool_call_id, .. } => Some(tool_call_id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_result_ids, vec!["slow", "fast"]);

    let guard = events.lock().expect("events");
    let tool_end_ids = guard
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ToolExecutionEnd { tool_call_id, .. } => Some(tool_call_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_end_ids, vec!["fast", "slow"]);
}

#[tokio::test]
async fn invalid_tool_json_becomes_error_tool_result() {
    let provider = Arc::new(FakeProvider::new(vec![
        vec![
            RawStreamEvent::ToolStart {
                content_index: 0,
                call_index: 0,
                id: "bad".to_string(),
                name: "read".to_string(),
            },
            RawStreamEvent::ToolArgs {
                content_index: 0,
                call_index: 0,
                delta: "{not-json".to_string(),
            },
            RawStreamEvent::ToolEnd {
                content_index: 0,
                call_index: 0,
            },
            RawStreamEvent::Done(Outcome::Normal),
        ],
        vec![
            RawStreamEvent::Text("recovered".to_string()),
            RawStreamEvent::Done(Outcome::Normal),
        ],
    ]));
    let (_control, receivers) = ControlHandle::new();
    let completion = run_agent_loop(
        provider,
        AgentLoopRequest {
            model_provider: "fake".to_string(),
            model: "fake".to_string(),
            generation_metadata: json!({}),
            system_instructions: Vec::new(),
            previous_messages: vec![],
            context_messages: Vec::new(),
            prompt_messages: vec![user_text_message("run")],
            tools: vec![Arc::new(DelayTool)],
            max_turns: 4,
        },
        Arc::new(RecordingSink::default()),
        receivers,
    )
    .await
    .expect("loop");
    let error_result = completion.messages.iter().any(|message| {
        matches!(
            message,
            Message::ToolResult {
                tool_call_id,
                is_error: true,
                ..
            } if tool_call_id == "bad"
        )
    });
    assert!(error_result);
}

#[tokio::test]
async fn graceful_stop_finishes_current_turn() {
    let provider = Arc::new(FakeProvider::new(vec![vec![
        RawStreamEvent::Text("one turn".to_string()),
        RawStreamEvent::Done(Outcome::Normal),
    ]]));
    let (control, receivers) = ControlHandle::new();
    control.stop();
    let completion = run_agent_loop(
        provider,
        AgentLoopRequest {
            model_provider: "fake".to_string(),
            model: "fake".to_string(),
            generation_metadata: json!({}),
            system_instructions: Vec::new(),
            previous_messages: vec![],
            context_messages: Vec::new(),
            prompt_messages: vec![user_text_message("run")],
            tools: vec![],
            max_turns: 4,
        },
        Arc::new(RecordingSink::default()),
        receivers,
    )
    .await
    .expect("loop");
    assert_eq!(completion.outcome, Outcome::Stopped);
}

#[tokio::test]
async fn abort_before_generation_returns_aborted() {
    let provider = Arc::new(FakeProvider::new(vec![vec![
        RawStreamEvent::Text("unused".to_string()),
        RawStreamEvent::Done(Outcome::Normal),
    ]]));
    let (control, receivers) = ControlHandle::new();
    control.abort();
    let completion = run_agent_loop(
        provider,
        AgentLoopRequest {
            model_provider: "fake".to_string(),
            model: "fake".to_string(),
            generation_metadata: json!({}),
            system_instructions: Vec::new(),
            previous_messages: vec![],
            context_messages: Vec::new(),
            prompt_messages: vec![user_text_message("run")],
            tools: vec![],
            max_turns: 4,
        },
        Arc::new(RecordingSink::default()),
        receivers,
    )
    .await
    .expect("loop");
    assert_eq!(completion.outcome, Outcome::Aborted);
    assert!(completion.messages.is_empty());
}

#[tokio::test]
async fn event_sink_failure_fails_invocation() {
    let provider = Arc::new(FakeProvider::new(vec![vec![
        RawStreamEvent::Text("hello".to_string()),
        RawStreamEvent::Done(Outcome::Normal),
    ]]));
    let (_control, receivers) = ControlHandle::new();
    let err = run_agent_loop(
        provider,
        AgentLoopRequest {
            model_provider: "fake".to_string(),
            model: "fake".to_string(),
            generation_metadata: json!({}),
            system_instructions: Vec::new(),
            previous_messages: vec![],
            context_messages: Vec::new(),
            prompt_messages: vec![user_text_message("run")],
            tools: vec![],
            max_turns: 4,
        },
        Arc::new(RecordingSink {
            events: Arc::new(Mutex::new(Vec::new())),
            fail_on_message_end: true,
        }),
        receivers,
    )
    .await
    .expect_err("sink failure");
    assert!(err.to_string().contains("event sink"));
}
