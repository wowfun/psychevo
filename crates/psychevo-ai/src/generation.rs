use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::{
    AbortHandle, AbortSignal, AdapterCall, AdapterContext, CredentialBindings, CredentialRequest,
    CredentialResolver, GenerationEvent, GenerationOutcome, GenerationOutput, GenerationSnapshot,
    LanguageAdapter, LanguageAdapterEvent, LanguageRequest, ModelDescriptor, ModelProfile,
    ProviderError, ProviderTool, RequestHeaders, TimeoutPolicy, ToolArgumentError,
    ToolArgumentErrorKind, ToolCall, Usage, Warning, abort_pair,
};
use crate::{AssistantContent, AssistantMessage, ErrorKind, ErrorPhase, Extensions, TextContent};

pub type SharedGenerationResult = Result<Arc<GenerationOutput>, Arc<GenerationError>>;
type EventItem = Result<GenerationEvent, GenerationError>;

const MAX_PENDING_GENERATION_EVENTS: usize = 256;
const MAX_PENDING_GENERATION_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Error)]
#[error("{error}")]
pub struct GenerationError {
    #[serde(flatten)]
    pub error: ProviderError,
    pub partial: GenerationSnapshot,
}

impl GenerationError {
    pub fn new(error: ProviderError, partial: GenerationSnapshot) -> Self {
        Self { error, partial }
    }
}

#[derive(Clone)]
pub struct CompletionHandle {
    completion: watch::Receiver<Option<SharedGenerationResult>>,
}

impl std::fmt::Debug for CompletionHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CompletionHandle { .. }")
    }
}

impl CompletionHandle {
    pub async fn wait(&self) -> SharedGenerationResult {
        let mut completion = self.completion.clone();
        loop {
            if let Some(result) = completion.borrow().clone() {
                return result;
            }
            if completion.changed().await.is_err() {
                if let Some(result) = completion.borrow().clone() {
                    return result;
                }
                return Err(Arc::new(GenerationError::new(
                    ProviderError::protocol("generation task ended without a completion result"),
                    GenerationSnapshot::empty(ModelDescriptor {
                        deployment_id: "unknown".to_string(),
                        provider_family: "unknown".to_string(),
                        capability: crate::Capability::Language,
                        model_id: "unknown".to_string(),
                        protocol_id: "unknown".to_string(),
                    }),
                )));
            }
        }
    }
}

/// A live language-generation invocation and its normalized event stream.
///
/// Slow consumers receive a bounded, coalescing stream. When incremental
/// events exceed the bound, a `Resync` event replaces them with the latest
/// authoritative snapshot.
pub struct Generation {
    events: EventReceiver,
    completion: CompletionHandle,
    abort: AbortHandle,
    owns_invocation: bool,
}

struct EventQueueState {
    pending: VecDeque<EventItem>,
    non_snapshot_bytes: usize,
    closed: bool,
}

struct EventSender {
    state: Arc<Mutex<EventQueueState>>,
    signal: mpsc::Sender<()>,
}

struct EventReceiver {
    state: Arc<Mutex<EventQueueState>>,
    signal_tx: mpsc::Sender<()>,
    signal_rx: mpsc::Receiver<()>,
}

fn event_channel() -> (EventSender, EventReceiver) {
    let state = Arc::new(Mutex::new(EventQueueState {
        pending: VecDeque::new(),
        non_snapshot_bytes: 0,
        closed: false,
    }));
    let (signal_tx, signal_rx) = mpsc::channel(1);
    (
        EventSender {
            state: Arc::clone(&state),
            signal: signal_tx.clone(),
        },
        EventReceiver {
            state,
            signal_tx,
            signal_rx,
        },
    )
}

impl EventSender {
    fn send(&self, item: EventItem, snapshot: Option<GenerationSnapshot>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.closed {
            return;
        }
        let essential = event_is_essential(&item);
        let updated_resync = update_pending_resync(&mut state, snapshot.as_ref());
        if !essential && updated_resync {
            drop(state);
            let _ = self.signal.try_send(());
            return;
        }
        if !essential && coalesce_event(&mut state, &item) {
            if state.non_snapshot_bytes > MAX_PENDING_GENERATION_BYTES {
                let dropped = drop_incremental_events(&mut state);
                if let Some(snapshot) = snapshot {
                    insert_or_update_resync(&mut state, snapshot, dropped.max(1));
                }
            }
            drop(state);
            let _ = self.signal.try_send(());
            return;
        }
        let item_bytes = event_payload_bytes(&item);
        if state.pending.len() + 1 > MAX_PENDING_GENERATION_EVENTS
            || state.non_snapshot_bytes.saturating_add(item_bytes) > MAX_PENDING_GENERATION_BYTES
        {
            let dropped = drop_incremental_events(&mut state);
            if let Some(snapshot) = snapshot {
                insert_or_update_resync(&mut state, snapshot, dropped.max(1));
            }
            if !essential {
                drop(state);
                let _ = self.signal.try_send(());
                return;
            }
        }
        state.non_snapshot_bytes = state.non_snapshot_bytes.saturating_add(item_bytes);
        state.pending.push_back(item);
        drop(state);
        let _ = self.signal.try_send(());
    }
}

impl Drop for EventSender {
    fn drop(&mut self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.closed = true;
        drop(state);
        let _ = self.signal.try_send(());
    }
}

impl EventReceiver {
    async fn recv(&mut self) -> Option<EventItem> {
        loop {
            if let Some(item) = self.pop_front() {
                return Some(item);
            }
            if self.is_closed() {
                return None;
            }
            let _ = self.signal_rx.recv().await;
        }
    }

    fn pop_front(&mut self) -> Option<EventItem> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let item = state.pending.pop_front()?;
        state.non_snapshot_bytes = state
            .non_snapshot_bytes
            .saturating_sub(event_payload_bytes(&item));
        let has_more = !state.pending.is_empty();
        drop(state);
        if has_more {
            let _ = self.signal_tx.try_send(());
        }
        Some(item)
    }

    fn is_closed(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .closed
    }

    fn poll_recv(&mut self, context: &mut Context<'_>) -> Poll<Option<EventItem>> {
        loop {
            if let Some(item) = self.pop_front() {
                return Poll::Ready(Some(item));
            }
            if self.is_closed() {
                return Poll::Ready(None);
            }
            match Pin::new(&mut self.signal_rx).poll_recv(context) {
                Poll::Ready(Some(_)) => continue,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

fn event_is_essential(item: &EventItem) -> bool {
    matches!(
        item,
        Err(_) | Ok(GenerationEvent::Started { .. }) | Ok(GenerationEvent::Finish { .. })
    )
}

fn event_payload_bytes(item: &EventItem) -> usize {
    match item {
        Ok(GenerationEvent::Resync { .. }) | Err(_) => 0,
        Ok(event) => serde_json::to_vec(event)
            .map(|bytes| bytes.len())
            .unwrap_or(0),
    }
}

fn update_pending_resync(
    state: &mut EventQueueState,
    snapshot: Option<&GenerationSnapshot>,
) -> bool {
    let Some(snapshot) = snapshot else {
        return false;
    };
    let Some(Ok(GenerationEvent::Resync {
        snapshot: retained,
        dropped_events,
    })) = state
        .pending
        .iter_mut()
        .find(|item| matches!(item, Ok(GenerationEvent::Resync { .. })))
    else {
        return false;
    };
    *retained = snapshot.clone();
    *dropped_events = dropped_events.saturating_add(1);
    true
}

fn insert_or_update_resync(
    state: &mut EventQueueState,
    snapshot: GenerationSnapshot,
    dropped: u64,
) {
    if let Some(Ok(GenerationEvent::Resync {
        snapshot: retained,
        dropped_events,
    })) = state
        .pending
        .iter_mut()
        .find(|item| matches!(item, Ok(GenerationEvent::Resync { .. })))
    {
        *retained = snapshot;
        *dropped_events = dropped_events.saturating_add(dropped);
        return;
    }
    state.pending.push_back(Ok(GenerationEvent::Resync {
        snapshot,
        dropped_events: dropped,
    }));
}

fn drop_incremental_events(state: &mut EventQueueState) -> u64 {
    let mut dropped = 0u64;
    let mut retained = VecDeque::new();
    while let Some(item) = state.pending.pop_front() {
        if event_is_essential(&item) {
            retained.push_back(item);
        } else {
            dropped = dropped.saturating_add(match item {
                Ok(GenerationEvent::Resync { dropped_events, .. }) => {
                    dropped_events.saturating_add(1)
                }
                _ => 1,
            });
        }
    }
    state.pending = retained;
    state.non_snapshot_bytes = state.pending.iter().map(event_payload_bytes).sum();
    dropped
}

fn coalesce_event(state: &mut EventQueueState, incoming: &EventItem) -> bool {
    let Ok(incoming) = incoming else {
        return false;
    };
    let replacement_index = match incoming {
        GenerationEvent::Usage { .. } => state
            .pending
            .iter()
            .rposition(|item| matches!(item, Ok(GenerationEvent::Usage { .. }))),
        GenerationEvent::Metadata { .. } => state
            .pending
            .iter()
            .rposition(|item| matches!(item, Ok(GenerationEvent::Metadata { .. }))),
        _ => state.pending.len().checked_sub(1),
    };
    let Some(index) = replacement_index else {
        return false;
    };
    let Some(Ok(retained)) = state.pending.get_mut(index) else {
        return false;
    };
    let old_bytes = serde_json::to_vec(retained)
        .map(|bytes| bytes.len())
        .unwrap_or(0);
    let merged = match (retained, incoming) {
        (
            GenerationEvent::TextDelta {
                content_index: retained_index,
                delta: retained,
            },
            GenerationEvent::TextDelta {
                content_index,
                delta,
            },
        ) if retained_index == content_index => {
            retained.push_str(delta);
            true
        }
        (
            GenerationEvent::ReasoningDelta {
                content_index: retained_index,
                delta: retained_delta,
                provider_evidence: retained_evidence,
            },
            GenerationEvent::ReasoningDelta {
                content_index,
                delta,
                provider_evidence,
            },
        ) if retained_index == content_index => {
            retained_delta.push_str(delta);
            if let Some(evidence) = provider_evidence.clone() {
                merge_provider_evidence(retained_evidence, evidence);
            }
            true
        }
        (
            GenerationEvent::ToolCallArgumentsDelta {
                content_index: retained_index,
                delta: retained,
            },
            GenerationEvent::ToolCallArgumentsDelta {
                content_index,
                delta,
            },
        ) if retained_index == content_index => {
            retained.push_str(delta);
            true
        }
        (GenerationEvent::Usage { usage: retained }, GenerationEvent::Usage { usage }) => {
            *retained = usage.clone();
            true
        }
        (
            GenerationEvent::Metadata { metadata: retained },
            GenerationEvent::Metadata { metadata },
        ) => {
            retained.extend(metadata.clone());
            true
        }
        _ => false,
    };
    if merged {
        let new_bytes = serde_json::to_vec(
            state
                .pending
                .get(index)
                .and_then(|item| item.as_ref().ok())
                .expect("coalesced generation event"),
        )
        .map(|bytes| bytes.len())
        .unwrap_or(0);
        state.non_snapshot_bytes = state
            .non_snapshot_bytes
            .saturating_sub(old_bytes)
            .saturating_add(new_bytes);
    }
    merged
}

impl std::fmt::Debug for Generation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Generation")
            .field("aborted", &self.abort.is_aborted())
            .finish_non_exhaustive()
    }
}

impl Generation {
    pub async fn next_event(&mut self) -> Option<Result<GenerationEvent, GenerationError>> {
        self.events.recv().await
    }

    pub fn completion(&self) -> CompletionHandle {
        self.completion.clone()
    }

    pub fn abort(&self) -> bool {
        self.abort.abort()
    }

    pub(crate) fn abort_handle(&self) -> AbortHandle {
        self.abort.clone()
    }

    pub async fn finish(mut self) -> Result<GenerationOutput, GenerationError> {
        let result = self.completion.wait().await;
        self.owns_invocation = false;
        match result {
            Ok(output) => Ok((*output).clone()),
            Err(error) => Err((*error).clone()),
        }
    }
}

impl Stream for Generation {
    type Item = Result<GenerationEvent, GenerationError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.events.poll_recv(context)
    }
}

impl Drop for Generation {
    fn drop(&mut self) {
        if self.owns_invocation {
            self.abort.abort();
        }
    }
}

#[derive(Clone)]
pub(crate) struct LanguageInvocationTarget {
    pub descriptor: ModelDescriptor,
    pub profile: Option<ModelProfile>,
    pub endpoint: String,
    pub deployment_headers: RequestHeaders,
    pub credentials: CredentialBindings,
    pub client: reqwest::Client,
    pub resolver: Arc<dyn CredentialResolver>,
    pub timeout_policy: TimeoutPolicy,
    pub adapter: Arc<dyn LanguageAdapter>,
}

pub(crate) fn start_generation(
    target: LanguageInvocationTarget,
    request: LanguageRequest,
) -> Generation {
    let (event_tx, event_rx) = event_channel();
    let (completion_tx, completion_rx) = watch::channel(None);
    let (abort, abort_signal) = abort_pair();
    let descriptor = target.descriptor.clone();
    event_tx.send(
        Ok(GenerationEvent::Started {
            model: descriptor.clone(),
        }),
        None,
    );

    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(run_generation(
                target,
                request,
                abort_signal,
                event_tx,
                completion_tx,
            ));
        }
        Err(_) => {
            let error = GenerationError::new(
                ProviderError::runtime_unavailable(),
                GenerationSnapshot::empty(descriptor),
            );
            event_tx.send(Err(error.clone()), Some(error.partial.clone()));
            let _ = completion_tx.send(Some(Err(Arc::new(error))));
        }
    }

    Generation {
        events: event_rx,
        completion: CompletionHandle {
            completion: completion_rx,
        },
        abort,
        owns_invocation: true,
    }
}

async fn run_generation(
    target: LanguageInvocationTarget,
    request: LanguageRequest,
    mut abort: AbortSignal,
    events: EventSender,
    completion: watch::Sender<Option<SharedGenerationResult>>,
) {
    let mut accumulator = GenerationAccumulator::new(target.descriptor.clone());
    let total_deadline = target
        .timeout_policy
        .total_deadline()
        .map(|duration| Instant::now() + duration);
    let idle_timeout = target.timeout_policy.progress_idle_timeout();
    let mut idle_deadline = idle_timeout.map(|duration| Instant::now() + duration);

    let headers = match merge_safe_headers(&target.deployment_headers, &request.headers) {
        Ok(headers) => headers,
        Err(error) => {
            complete_error(error, &accumulator, &events, &completion);
            return;
        }
    };

    let credential_request = CredentialRequest {
        deployment_id: target.descriptor.deployment_id.clone(),
        provider_family: target.descriptor.provider_family.clone(),
        bindings: target.credentials.clone(),
    };
    let resolve = target.resolver.resolve(credential_request);
    tokio::pin!(resolve);
    let credentials = tokio::select! {
        biased;
        _ = abort.wait_for_abort() => {
            complete_aborted(&accumulator, &events, &completion);
            return;
        }
        _ = wait_for_deadline(total_deadline) => {
            complete_error(timeout_error("generation total deadline elapsed", ErrorPhase::Credentials), &accumulator, &events, &completion);
            return;
        }
        _ = wait_for_deadline(idle_deadline) => {
            complete_error(timeout_error("credential resolution made no progress", ErrorPhase::Credentials), &accumulator, &events, &completion);
            return;
        }
        result = &mut resolve => match result {
            Ok(credentials) => credentials,
            Err(error) => {
                complete_error(error, &accumulator, &events, &completion);
                return;
            }
        }
    };
    idle_deadline = idle_timeout.map(|duration| Instant::now() + duration);

    let context = AdapterContext {
        model: target.descriptor.clone(),
        profile: target.profile,
        endpoint: target.endpoint,
        headers,
        client: target.client,
        credentials,
        abort: abort.clone(),
        timeout_policy: target.timeout_policy.clone(),
    };
    let adapter_start = target.adapter.stream(AdapterCall {
        model: target.descriptor.model_id.clone(),
        request,
        context,
    });
    tokio::pin!(adapter_start);
    let mut stream = tokio::select! {
        biased;
        _ = abort.wait_for_abort() => {
            complete_aborted(&accumulator, &events, &completion);
            return;
        }
        _ = wait_for_deadline(total_deadline) => {
            complete_error(timeout_error("generation total deadline elapsed", ErrorPhase::Dispatch), &accumulator, &events, &completion);
            return;
        }
        _ = wait_for_deadline(idle_deadline) => {
            complete_error(timeout_error("provider dispatch made no progress", ErrorPhase::Dispatch), &accumulator, &events, &completion);
            return;
        }
        result = &mut adapter_start => match result {
            Ok(stream) => stream,
            Err(error) => {
                complete_error(error, &accumulator, &events, &completion);
                return;
            }
        }
    };
    idle_deadline = idle_timeout.map(|duration| Instant::now() + duration);

    loop {
        let next = tokio::select! {
            biased;
            _ = abort.wait_for_abort() => {
                complete_aborted(&accumulator, &events, &completion);
                return;
            }
            _ = wait_for_deadline(total_deadline) => {
                complete_error(timeout_error("generation total deadline elapsed", ErrorPhase::Stream), &accumulator, &events, &completion);
                return;
            }
            _ = wait_for_deadline(idle_deadline) => {
                complete_error(timeout_error("provider made no generation progress", ErrorPhase::Stream), &accumulator, &events, &completion);
                return;
            }
            next = stream.next() => next,
        };
        let Some(next) = next else {
            complete_error(
                ProviderError::protocol("provider stream ended before Finish"),
                &accumulator,
                &events,
                &completion,
            );
            return;
        };
        let adapter_event = match next {
            Ok(event) => event,
            Err(error) => {
                complete_error(error, &accumulator, &events, &completion);
                return;
            }
        };
        if adapter_event_is_progress(&adapter_event) {
            idle_deadline = idle_timeout.map(|duration| Instant::now() + duration);
        }
        match accumulator.accept(adapter_event) {
            Ok(AcceptedEvent::Public(event)) => {
                events.send(Ok(event), Some(accumulator.snapshot()));
            }
            Ok(AcceptedEvent::Finished(output, event)) => {
                events.send(Ok(event), Some(output.snapshot.clone()));
                let _ = completion.send(Some(Ok(Arc::new(*output))));
                return;
            }
            Err(error) => {
                complete_error(error, &accumulator, &events, &completion);
                return;
            }
        }
    }
}

fn timeout_error(summary: &str, phase: ErrorPhase) -> ProviderError {
    ProviderError::new(ErrorKind::Timeout, phase, summary)
}

fn complete_error(
    error: ProviderError,
    accumulator: &GenerationAccumulator,
    events: &EventSender,
    completion: &watch::Sender<Option<SharedGenerationResult>>,
) {
    let error = GenerationError::new(error, accumulator.snapshot());
    events.send(Err(error.clone()), Some(error.partial.clone()));
    let _ = completion.send(Some(Err(Arc::new(error))));
}

fn complete_aborted(
    accumulator: &GenerationAccumulator,
    events: &EventSender,
    completion: &watch::Sender<Option<SharedGenerationResult>>,
) {
    let output = GenerationOutput {
        snapshot: accumulator.snapshot(),
        outcome: GenerationOutcome::Aborted,
        finish_reason: None,
    };
    events.send(
        Ok(GenerationEvent::Finish {
            outcome: GenerationOutcome::Aborted,
            finish_reason: None,
        }),
        Some(output.snapshot.clone()),
    );
    let _ = completion.send(Some(Ok(Arc::new(output))));
}

async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

fn adapter_event_is_progress(event: &LanguageAdapterEvent) -> bool {
    !matches!(
        event,
        LanguageAdapterEvent::Usage { .. }
            | LanguageAdapterEvent::Metadata { .. }
            | LanguageAdapterEvent::Warning { .. }
    )
}

pub(crate) fn merge_safe_headers(
    deployment: &RequestHeaders,
    invocation: &RequestHeaders,
) -> Result<RequestHeaders, ProviderError> {
    let mut headers = RequestHeaders::new();
    for (name, value) in deployment.iter().chain(invocation) {
        validate_safe_header(name, value)?;
        headers.insert(name.to_ascii_lowercase(), value.clone());
    }
    Ok(headers)
}

fn validate_safe_header(name: &str, value: &str) -> Result<(), ProviderError> {
    let normalized = name.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || normalized
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || byte == b'-'))
    {
        return Err(ProviderError::configuration(format!(
            "invalid HTTP header name `{name}`"
        )));
    }
    if matches!(
        normalized.as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie" | "x-api-key" | "api-key"
    ) {
        return Err(ProviderError::configuration(format!(
            "authentication header `{name}` must be supplied through credential slots"
        )));
    }
    if value.contains('\r') || value.contains('\n') {
        return Err(ProviderError::configuration(format!(
            "HTTP header `{name}` contains a line break"
        )));
    }
    Ok(())
}

enum AcceptedEvent {
    Public(GenerationEvent),
    Finished(Box<GenerationOutput>, GenerationEvent),
}

#[derive(Clone)]
struct GenerationAccumulator {
    model: ModelDescriptor,
    contents: BTreeMap<usize, ContentState>,
    next_content_index: usize,
    usage: Option<Usage>,
    warnings: Vec<Warning>,
    provider_metadata: Extensions,
    finished: bool,
}

#[derive(Clone)]
enum ContentState {
    Text {
        text: String,
        closed: bool,
    },
    Reasoning {
        text: String,
        provider_evidence: Option<Value>,
        closed: bool,
    },
    ToolCall {
        id: String,
        name: String,
        arguments_delta: String,
        final_call: Option<ToolCall>,
    },
    ProviderTool {
        tool: ProviderTool,
        closed: bool,
    },
    Source {
        source: crate::AssistantSource,
    },
}

impl GenerationAccumulator {
    fn new(model: ModelDescriptor) -> Self {
        Self {
            model,
            contents: BTreeMap::new(),
            next_content_index: 0,
            usage: None,
            warnings: Vec::new(),
            provider_metadata: Extensions::new(),
            finished: false,
        }
    }

    fn accept(&mut self, event: LanguageAdapterEvent) -> Result<AcceptedEvent, ProviderError> {
        if self.finished {
            return Err(ProviderError::protocol(
                "provider emitted an event after Finish",
            ));
        }
        match event {
            LanguageAdapterEvent::TextStart { content_index } => {
                self.insert_content(
                    content_index,
                    ContentState::Text {
                        text: String::new(),
                        closed: false,
                    },
                )?;
                Ok(AcceptedEvent::Public(GenerationEvent::TextStart {
                    content_index,
                }))
            }
            LanguageAdapterEvent::TextDelta {
                content_index,
                delta,
            } => {
                match self.contents.get_mut(&content_index) {
                    Some(ContentState::Text {
                        text,
                        closed: false,
                    }) => text.push_str(&delta),
                    _ => return Err(content_lifecycle_error("text delta", content_index)),
                }
                Ok(AcceptedEvent::Public(GenerationEvent::TextDelta {
                    content_index,
                    delta,
                }))
            }
            LanguageAdapterEvent::TextEnd { content_index } => {
                match self.contents.get_mut(&content_index) {
                    Some(ContentState::Text { closed, .. }) if !*closed => *closed = true,
                    _ => return Err(content_lifecycle_error("text end", content_index)),
                }
                Ok(AcceptedEvent::Public(GenerationEvent::TextEnd {
                    content_index,
                }))
            }
            LanguageAdapterEvent::ReasoningStart { content_index } => {
                self.insert_content(
                    content_index,
                    ContentState::Reasoning {
                        text: String::new(),
                        provider_evidence: None,
                        closed: false,
                    },
                )?;
                Ok(AcceptedEvent::Public(GenerationEvent::ReasoningStart {
                    content_index,
                }))
            }
            LanguageAdapterEvent::ReasoningDelta {
                content_index,
                delta,
                provider_evidence,
            } => {
                match self.contents.get_mut(&content_index) {
                    Some(ContentState::Reasoning {
                        text,
                        provider_evidence: retained,
                        closed: false,
                    }) => {
                        text.push_str(&delta);
                        if let Some(fragment) = provider_evidence.clone() {
                            merge_provider_evidence(retained, fragment);
                        }
                    }
                    _ => return Err(content_lifecycle_error("reasoning delta", content_index)),
                }
                Ok(AcceptedEvent::Public(GenerationEvent::ReasoningDelta {
                    content_index,
                    delta,
                    provider_evidence,
                }))
            }
            LanguageAdapterEvent::ReasoningEnd { content_index } => {
                match self.contents.get_mut(&content_index) {
                    Some(ContentState::Reasoning { closed, .. }) if !*closed => *closed = true,
                    _ => return Err(content_lifecycle_error("reasoning end", content_index)),
                }
                Ok(AcceptedEvent::Public(GenerationEvent::ReasoningEnd {
                    content_index,
                }))
            }
            LanguageAdapterEvent::ToolCallStart {
                content_index,
                id,
                name,
            } => {
                if id.is_empty() || name.is_empty() {
                    return Err(ProviderError::protocol(
                        "tool-call start requires non-empty id and name",
                    ));
                }
                self.insert_content(
                    content_index,
                    ContentState::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments_delta: String::new(),
                        final_call: None,
                    },
                )?;
                Ok(AcceptedEvent::Public(GenerationEvent::ToolCallStart {
                    content_index,
                    id,
                    name,
                }))
            }
            LanguageAdapterEvent::ToolCallArgumentsDelta {
                content_index,
                delta,
            } => {
                match self.contents.get_mut(&content_index) {
                    Some(ContentState::ToolCall {
                        arguments_delta,
                        final_call: None,
                        ..
                    }) => arguments_delta.push_str(&delta),
                    _ => return Err(content_lifecycle_error("tool-call delta", content_index)),
                }
                Ok(AcceptedEvent::Public(
                    GenerationEvent::ToolCallArgumentsDelta {
                        content_index,
                        delta,
                    },
                ))
            }
            LanguageAdapterEvent::ToolCallEnd {
                content_index,
                arguments_raw,
            } => {
                let state = self
                    .contents
                    .get_mut(&content_index)
                    .ok_or_else(|| content_lifecycle_error("tool-call end", content_index))?;
                let ContentState::ToolCall {
                    id,
                    name,
                    final_call,
                    ..
                } = state
                else {
                    return Err(content_lifecycle_error("tool-call end", content_index));
                };
                if final_call.is_some() {
                    return Err(content_lifecycle_error(
                        "duplicate tool-call end",
                        content_index,
                    ));
                }
                let (arguments, argument_error) = parse_tool_arguments(&arguments_raw);
                *final_call = Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments_raw: arguments_raw.clone(),
                    arguments,
                    argument_error: argument_error.clone(),
                    extensions: Extensions::new(),
                });
                Ok(AcceptedEvent::Public(GenerationEvent::ToolCallEnd {
                    content_index,
                    arguments_raw,
                    argument_error,
                }))
            }
            LanguageAdapterEvent::ProviderToolStart {
                content_index,
                id,
                name,
                action,
            } => {
                if id.is_empty() || name.is_empty() {
                    return Err(ProviderError::protocol(
                        "provider-tool start requires non-empty id and name",
                    ));
                }
                let tool = ProviderTool {
                    id,
                    name,
                    action,
                    status: "running".to_string(),
                    extensions: Extensions::new(),
                };
                self.insert_content(
                    content_index,
                    ContentState::ProviderTool {
                        tool: tool.clone(),
                        closed: false,
                    },
                )?;
                Ok(AcceptedEvent::Public(GenerationEvent::ProviderToolStart {
                    content_index,
                    tool,
                }))
            }
            LanguageAdapterEvent::ProviderToolEnd {
                content_index,
                id,
                name,
                action,
                status,
            } => {
                let tool = match self.contents.get_mut(&content_index) {
                    Some(ContentState::ProviderTool { tool, closed }) if !*closed => {
                        if tool.id != id || tool.name != name {
                            return Err(ProviderError::protocol(format!(
                                "provider-tool identity changed at content index {content_index}"
                            )));
                        }
                        tool.action = action;
                        tool.status = status;
                        *closed = true;
                        tool.clone()
                    }
                    _ => {
                        return Err(content_lifecycle_error("provider-tool end", content_index));
                    }
                };
                Ok(AcceptedEvent::Public(GenerationEvent::ProviderToolEnd {
                    content_index,
                    tool,
                }))
            }
            LanguageAdapterEvent::Source {
                content_index,
                source,
            } => {
                self.insert_content(
                    content_index,
                    ContentState::Source {
                        source: source.clone(),
                    },
                )?;
                Ok(AcceptedEvent::Public(GenerationEvent::Source {
                    content_index,
                    source,
                }))
            }
            LanguageAdapterEvent::Usage { usage } => {
                self.usage = Some(usage.clone());
                Ok(AcceptedEvent::Public(GenerationEvent::Usage { usage }))
            }
            LanguageAdapterEvent::Metadata { metadata } => {
                self.provider_metadata.extend(metadata.clone());
                Ok(AcceptedEvent::Public(GenerationEvent::Metadata {
                    metadata,
                }))
            }
            LanguageAdapterEvent::Warning { warning } => {
                self.warnings.push(warning.clone());
                Ok(AcceptedEvent::Public(GenerationEvent::Warning { warning }))
            }
            LanguageAdapterEvent::Finish { finish_reason } => {
                self.validate_all_closed()?;
                self.finished = true;
                let output = GenerationOutput {
                    snapshot: self.snapshot(),
                    outcome: GenerationOutcome::Completed,
                    finish_reason: finish_reason.clone(),
                };
                Ok(AcceptedEvent::Finished(
                    Box::new(output),
                    GenerationEvent::Finish {
                        outcome: GenerationOutcome::Completed,
                        finish_reason,
                    },
                ))
            }
        }
    }

    fn insert_content(
        &mut self,
        content_index: usize,
        content: ContentState,
    ) -> Result<(), ProviderError> {
        if content_index != self.next_content_index {
            return Err(ProviderError::protocol(format!(
                "expected content index {}, received {content_index}",
                self.next_content_index
            )));
        }
        if self.contents.insert(content_index, content).is_some() {
            return Err(ProviderError::protocol(format!(
                "duplicate content index {content_index}"
            )));
        }
        self.next_content_index += 1;
        Ok(())
    }

    fn validate_all_closed(&self) -> Result<(), ProviderError> {
        for (index, content) in &self.contents {
            let closed = match content {
                ContentState::Text { closed, .. }
                | ContentState::Reasoning { closed, .. }
                | ContentState::ProviderTool { closed, .. } => *closed,
                ContentState::ToolCall { final_call, .. } => final_call.is_some(),
                ContentState::Source { .. } => true,
            };
            if !closed {
                return Err(ProviderError::protocol(format!(
                    "content index {index} was still open at Finish"
                )));
            }
        }
        Ok(())
    }

    fn snapshot(&self) -> GenerationSnapshot {
        let content = self
            .contents
            .values()
            .map(ContentState::to_assistant_content)
            .collect();
        GenerationSnapshot {
            model: self.model.clone(),
            assistant: AssistantMessage {
                content,
                extensions: Extensions::new(),
            },
            usage: self.usage.clone(),
            warnings: self.warnings.clone(),
            provider_metadata: self.provider_metadata.clone(),
        }
    }
}

impl ContentState {
    fn to_assistant_content(&self) -> AssistantContent {
        match self {
            Self::Text { text, .. } => AssistantContent::Text(TextContent::new(text)),
            Self::Reasoning {
                text,
                provider_evidence,
                ..
            } => AssistantContent::Reasoning {
                text: text.clone(),
                provider_evidence: provider_evidence.clone(),
            },
            Self::ToolCall {
                id,
                name,
                arguments_delta,
                final_call,
            } => AssistantContent::ToolCall(final_call.clone().unwrap_or_else(|| ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments_raw: arguments_delta.clone(),
                arguments: None,
                argument_error: Some(ToolArgumentError {
                    kind: ToolArgumentErrorKind::Incomplete,
                    message: "tool arguments did not complete".to_string(),
                }),
                extensions: Extensions::new(),
            })),
            Self::ProviderTool { tool, .. } => AssistantContent::ProviderTool(tool.clone()),
            Self::Source { source } => AssistantContent::Source {
                source: source.clone(),
            },
        }
    }
}

fn parse_tool_arguments(arguments_raw: &str) -> (Option<Value>, Option<ToolArgumentError>) {
    match serde_json::from_str::<Value>(arguments_raw) {
        Ok(Value::Object(object)) => (Some(Value::Object(object)), None),
        Ok(_) => (
            None,
            Some(ToolArgumentError {
                kind: ToolArgumentErrorKind::NotAnObject,
                message: "tool arguments must be a JSON object".to_string(),
            }),
        ),
        Err(error) => (
            None,
            Some(ToolArgumentError {
                kind: ToolArgumentErrorKind::InvalidJson,
                message: format!("invalid tool argument JSON: {error}"),
            }),
        ),
    }
}

fn merge_provider_evidence(retained: &mut Option<Value>, fragment: Value) {
    let Some(current) = retained.as_mut() else {
        *retained = Some(fragment);
        return;
    };
    match (current, fragment) {
        (Value::Object(current), Value::Object(fragment)) => {
            for (key, value) in fragment {
                match current.get_mut(&key) {
                    Some(existing) => {
                        let mut retained = Some(std::mem::take(existing));
                        merge_provider_evidence(&mut retained, value);
                        *existing = retained.expect("merged provider evidence");
                    }
                    None => {
                        current.insert(key, value);
                    }
                }
            }
        }
        (Value::Array(current), Value::Array(mut fragment)) => current.append(&mut fragment),
        (Value::String(current), Value::String(fragment)) => current.push_str(&fragment),
        (current, fragment) => *current = fragment,
    }
}

fn content_lifecycle_error(action: &str, content_index: usize) -> ProviderError {
    ProviderError::protocol(format!(
        "invalid {action} lifecycle at content index {content_index}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> ModelDescriptor {
        ModelDescriptor {
            deployment_id: "fake".to_string(),
            provider_family: "fake".to_string(),
            capability: crate::Capability::Language,
            model_id: "test".to_string(),
            protocol_id: "fake".to_string(),
        }
    }

    #[test]
    fn invalid_tool_arguments_are_retained_without_failing_generation() {
        let mut accumulator = GenerationAccumulator::new(descriptor());
        accumulator
            .accept(LanguageAdapterEvent::ToolCallStart {
                content_index: 0,
                id: "call-1".to_string(),
                name: "read".to_string(),
            })
            .unwrap();
        accumulator
            .accept(LanguageAdapterEvent::ToolCallArgumentsDelta {
                content_index: 0,
                delta: "{\"path\":".to_string(),
            })
            .unwrap();
        let event = accumulator
            .accept(LanguageAdapterEvent::ToolCallEnd {
                content_index: 0,
                arguments_raw: "{\"path\":".to_string(),
            })
            .unwrap();
        assert!(matches!(
            event,
            AcceptedEvent::Public(GenerationEvent::ToolCallEnd {
                argument_error: Some(ToolArgumentError {
                    kind: ToolArgumentErrorKind::InvalidJson,
                    ..
                }),
                ..
            })
        ));
        assert!(
            accumulator
                .accept(LanguageAdapterEvent::Finish {
                    finish_reason: None,
                })
                .is_ok()
        );
    }

    #[test]
    fn reasoning_provider_evidence_accumulates_signature_fragments() {
        let mut accumulator = GenerationAccumulator::new(descriptor());
        accumulator
            .accept(LanguageAdapterEvent::ReasoningStart { content_index: 0 })
            .unwrap();
        for signature in ["signed-", "thinking"] {
            accumulator
                .accept(LanguageAdapterEvent::ReasoningDelta {
                    content_index: 0,
                    delta: String::new(),
                    provider_evidence: Some(serde_json::json!({
                        "signature": signature,
                    })),
                })
                .unwrap();
        }
        accumulator
            .accept(LanguageAdapterEvent::ReasoningEnd { content_index: 0 })
            .unwrap();
        let AcceptedEvent::Finished(output, _) = accumulator
            .accept(LanguageAdapterEvent::Finish {
                finish_reason: None,
            })
            .unwrap()
        else {
            panic!("finished output");
        };
        let AssistantContent::Reasoning {
            provider_evidence, ..
        } = &output.snapshot.assistant.content[0]
        else {
            panic!("reasoning block");
        };
        assert_eq!(
            provider_evidence.as_ref().expect("evidence")["signature"],
            "signed-thinking"
        );
    }

    #[tokio::test]
    async fn slow_consumer_receives_bounded_resync_and_terminal_snapshot() {
        let (events, mut receiver) = event_channel();
        let mut accumulator = GenerationAccumulator::new(descriptor());
        events.send(
            Ok(GenerationEvent::Started {
                model: descriptor(),
            }),
            None,
        );
        let AcceptedEvent::Public(start) = accumulator
            .accept(LanguageAdapterEvent::TextStart { content_index: 0 })
            .expect("text start")
        else {
            panic!("public start");
        };
        events.send(Ok(start), Some(accumulator.snapshot()));
        for _ in 0..400 {
            let AcceptedEvent::Public(delta) = accumulator
                .accept(LanguageAdapterEvent::TextDelta {
                    content_index: 0,
                    delta: "x".repeat(1024),
                })
                .expect("text delta")
            else {
                panic!("public delta");
            };
            events.send(Ok(delta), Some(accumulator.snapshot()));
        }
        let AcceptedEvent::Public(end) = accumulator
            .accept(LanguageAdapterEvent::TextEnd { content_index: 0 })
            .expect("text end")
        else {
            panic!("public end");
        };
        events.send(Ok(end), Some(accumulator.snapshot()));
        let AcceptedEvent::Finished(output, finish) = accumulator
            .accept(LanguageAdapterEvent::Finish {
                finish_reason: None,
            })
            .expect("finish")
        else {
            panic!("finished output");
        };
        events.send(Ok(finish), Some(output.snapshot.clone()));

        {
            let state = events
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            assert!(state.pending.len() <= MAX_PENDING_GENERATION_EVENTS);
            assert!(state.non_snapshot_bytes <= MAX_PENDING_GENERATION_BYTES);
        }
        drop(events);

        let mut saw_started = false;
        let mut saw_finish = false;
        let mut resync = None;
        while let Some(event) = receiver.recv().await {
            match event.expect("public event") {
                GenerationEvent::Started { .. } => saw_started = true,
                GenerationEvent::Resync {
                    snapshot,
                    dropped_events,
                } => resync = Some((snapshot, dropped_events)),
                GenerationEvent::Finish { .. } => saw_finish = true,
                _ => {}
            }
        }
        let (snapshot, dropped_events) = resync.expect("resync");
        let AssistantContent::Text(text) = &snapshot.assistant.content[0] else {
            panic!("text snapshot");
        };
        assert_eq!(text.text.len(), 400 * 1024);
        assert!(dropped_events > 0);
        assert!(saw_started);
        assert!(saw_finish);
        assert_eq!(snapshot, output.snapshot);
    }
}
