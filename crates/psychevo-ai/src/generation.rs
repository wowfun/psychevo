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
    pending: VecDeque<QueuedEvent>,
    non_snapshot_bytes: usize,
    closed: bool,
    accumulator: GenerationAccumulator,
}

enum QueuedEvent {
    Item { item: Box<EventItem>, bytes: usize },
    ResyncMarker { dropped_events: u64 },
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

fn event_channel(model: ModelDescriptor) -> (EventSender, EventReceiver) {
    let state = Arc::new(Mutex::new(EventQueueState {
        pending: VecDeque::new(),
        non_snapshot_bytes: 0,
        closed: false,
        accumulator: GenerationAccumulator::new(model),
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
    fn accept(
        &self,
        event: LanguageAdapterEvent,
    ) -> Result<Option<Box<GenerationOutput>>, ProviderError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let accepted = state.accumulator.accept(event)?;
        let output = match accepted {
            AcceptedEvent::Public(event) => {
                enqueue_item(&mut state, Ok(event));
                None
            }
            AcceptedEvent::Finished(output, event) => {
                enqueue_item(&mut state, Ok(event));
                Some(output)
            }
        };
        drop(state);
        let _ = self.signal.try_send(());
        Ok(output)
    }

    fn snapshot(&self) -> GenerationSnapshot {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .accumulator
            .snapshot()
    }

    fn send_item(&self, item: EventItem) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        enqueue_item(&mut state, item);
        drop(state);
        let _ = self.signal.try_send(());
    }
}

fn enqueue_item(state: &mut EventQueueState, item: EventItem) {
    if state.closed {
        return;
    }
    let essential = event_is_essential(&item);
    if !essential && increment_pending_resync(state, 1) {
        return;
    }
    let item_bytes = event_payload_bytes(&item);
    if !essential && coalesce_event(state, &item, item_bytes) {
        if state.non_snapshot_bytes > MAX_PENDING_GENERATION_BYTES {
            let dropped = drop_incremental_events(state);
            insert_or_update_resync(state, dropped.max(1));
        }
        return;
    }
    if state.pending.len() + 1 > MAX_PENDING_GENERATION_EVENTS
        || state.non_snapshot_bytes.saturating_add(item_bytes) > MAX_PENDING_GENERATION_BYTES
    {
        let dropped = drop_incremental_events(state);
        insert_or_update_resync(state, dropped.max(1));
        if !essential {
            return;
        }
    }
    state.non_snapshot_bytes = state.non_snapshot_bytes.saturating_add(item_bytes);
    state.pending.push_back(QueuedEvent::Item {
        item: Box::new(item),
        bytes: item_bytes,
    });
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
        let queued = state.pending.pop_front()?;
        let item = match queued {
            QueuedEvent::Item { item, bytes } => {
                state.non_snapshot_bytes = state.non_snapshot_bytes.saturating_sub(bytes);
                *item
            }
            QueuedEvent::ResyncMarker { dropped_events } => Ok(GenerationEvent::Resync {
                snapshot: Box::new(state.accumulator.snapshot()),
                dropped_events,
            }),
        };
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

fn increment_pending_resync(state: &mut EventQueueState, dropped: u64) -> bool {
    let Some(QueuedEvent::ResyncMarker { dropped_events }) = state
        .pending
        .iter_mut()
        .find(|item| matches!(item, QueuedEvent::ResyncMarker { .. }))
    else {
        return false;
    };
    *dropped_events = dropped_events.saturating_add(dropped);
    true
}

fn insert_or_update_resync(state: &mut EventQueueState, dropped: u64) {
    if increment_pending_resync(state, dropped) {
        return;
    }
    state.pending.push_back(QueuedEvent::ResyncMarker {
        dropped_events: dropped,
    });
}

fn drop_incremental_events(state: &mut EventQueueState) -> u64 {
    let mut dropped = 0u64;
    let mut retained = VecDeque::new();
    while let Some(queued) = state.pending.pop_front() {
        match queued {
            QueuedEvent::Item { ref item, .. } if event_is_essential(item.as_ref()) => {
                retained.push_back(queued);
            }
            QueuedEvent::Item { .. } => dropped = dropped.saturating_add(1),
            QueuedEvent::ResyncMarker { dropped_events } => {
                dropped = dropped.saturating_add(dropped_events);
            }
        }
    }
    state.pending = retained;
    state.non_snapshot_bytes = state
        .pending
        .iter()
        .map(|queued| match queued {
            QueuedEvent::Item { bytes, .. } => *bytes,
            QueuedEvent::ResyncMarker { .. } => 0,
        })
        .sum();
    dropped
}

fn coalesce_event(
    state: &mut EventQueueState,
    incoming: &EventItem,
    incoming_bytes: usize,
) -> bool {
    let Ok(incoming) = incoming else {
        return false;
    };
    let replacement_index = match incoming {
        GenerationEvent::Usage { .. } => state.pending.iter().rposition(|item| {
            matches!(item, QueuedEvent::Item { item, .. }
                if matches!(item.as_ref(), Ok(GenerationEvent::Usage { .. })))
        }),
        GenerationEvent::Metadata { .. } => state.pending.iter().rposition(|item| {
            matches!(item, QueuedEvent::Item { item, .. }
                if matches!(item.as_ref(), Ok(GenerationEvent::Metadata { .. })))
        }),
        _ => state.pending.len().checked_sub(1),
    };
    let Some(index) = replacement_index else {
        return false;
    };
    let Some(QueuedEvent::Item {
        item,
        bytes: retained_bytes,
    }) = state.pending.get_mut(index)
    else {
        return false;
    };
    let Ok(retained) = item.as_mut() else {
        return false;
    };
    let old_bytes = *retained_bytes;
    let mut growth = 0usize;
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
            growth = encoded_string_content_bytes(delta);
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
            growth = encoded_string_content_bytes(delta);
            if let Some(evidence) = provider_evidence.clone() {
                growth = growth.saturating_add(
                    serde_json::to_vec(&evidence)
                        .map(|bytes| bytes.len())
                        .unwrap_or(0),
                );
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
            growth = encoded_string_content_bytes(delta);
            true
        }
        (GenerationEvent::Usage { usage: retained }, GenerationEvent::Usage { usage }) => {
            *retained = usage.clone();
            *retained_bytes = incoming_bytes;
            true
        }
        (
            GenerationEvent::Metadata { metadata: retained },
            GenerationEvent::Metadata { metadata },
        ) => {
            retained.extend(metadata.clone());
            growth = incoming_bytes;
            true
        }
        _ => false,
    };
    if merged {
        if !matches!(incoming, GenerationEvent::Usage { .. }) {
            *retained_bytes = retained_bytes.saturating_add(growth);
        }
        state.non_snapshot_bytes = state
            .non_snapshot_bytes
            .saturating_sub(old_bytes)
            .saturating_add(*retained_bytes);
    }
    merged
}

fn encoded_string_content_bytes(value: &str) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len().saturating_sub(2))
        .unwrap_or(value.len())
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
    let descriptor = target.descriptor.clone();
    let (event_tx, event_rx) = event_channel(descriptor.clone());
    let (completion_tx, completion_rx) = watch::channel(None);
    let (abort, abort_signal) = abort_pair();
    event_tx.send_item(Ok(GenerationEvent::Started {
        model: descriptor.clone(),
    }));

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
            let error =
                GenerationError::new(ProviderError::runtime_unavailable(), event_tx.snapshot());
            event_tx.send_item(Err(error.clone()));
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
    let total_deadline = target
        .timeout_policy
        .total_deadline()
        .map(|duration| Instant::now() + duration);
    let idle_timeout = target.timeout_policy.progress_idle_timeout();
    let mut idle_deadline = idle_timeout.map(|duration| Instant::now() + duration);

    let headers = match merge_safe_headers(&target.deployment_headers, &request.headers) {
        Ok(headers) => headers,
        Err(error) => {
            complete_error(error, &events, &completion);
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
            complete_aborted(&events, &completion);
            return;
        }
        _ = wait_for_deadline(total_deadline) => {
            complete_error(timeout_error("generation total deadline elapsed", ErrorPhase::Credentials), &events, &completion);
            return;
        }
        _ = wait_for_deadline(idle_deadline) => {
            complete_error(timeout_error("credential resolution made no progress", ErrorPhase::Credentials), &events, &completion);
            return;
        }
        result = &mut resolve => match result {
            Ok(credentials) => credentials,
            Err(error) => {
                complete_error(error, &events, &completion);
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
            complete_aborted(&events, &completion);
            return;
        }
        _ = wait_for_deadline(total_deadline) => {
            complete_error(timeout_error("generation total deadline elapsed", ErrorPhase::Dispatch), &events, &completion);
            return;
        }
        _ = wait_for_deadline(idle_deadline) => {
            complete_error(timeout_error("provider dispatch made no progress", ErrorPhase::Dispatch), &events, &completion);
            return;
        }
        result = &mut adapter_start => match result {
            Ok(stream) => stream,
            Err(error) => {
                complete_error(error, &events, &completion);
                return;
            }
        }
    };
    idle_deadline = idle_timeout.map(|duration| Instant::now() + duration);

    loop {
        let next = tokio::select! {
            biased;
            _ = abort.wait_for_abort() => {
                complete_aborted(&events, &completion);
                return;
            }
            _ = wait_for_deadline(total_deadline) => {
                complete_error(timeout_error("generation total deadline elapsed", ErrorPhase::Stream), &events, &completion);
                return;
            }
            _ = wait_for_deadline(idle_deadline) => {
                complete_error(timeout_error("provider made no generation progress", ErrorPhase::Stream), &events, &completion);
                return;
            }
            next = stream.next() => next,
        };
        let Some(next) = next else {
            complete_error(
                ProviderError::protocol("provider stream ended before Finish"),
                &events,
                &completion,
            );
            return;
        };
        let adapter_event = match next {
            Ok(event) => event,
            Err(error) => {
                complete_error(error, &events, &completion);
                return;
            }
        };
        if adapter_event_is_progress(&adapter_event) {
            idle_deadline = idle_timeout.map(|duration| Instant::now() + duration);
        }
        match events.accept(adapter_event) {
            Ok(None) => {}
            Ok(Some(output)) => {
                let _ = completion.send(Some(Ok(Arc::new(*output))));
                return;
            }
            Err(error) => {
                complete_error(error, &events, &completion);
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
    events: &EventSender,
    completion: &watch::Sender<Option<SharedGenerationResult>>,
) {
    let error = GenerationError::new(error, events.snapshot());
    events.send_item(Err(error.clone()));
    let _ = completion.send(Some(Err(Arc::new(error))));
}

fn complete_aborted(
    events: &EventSender,
    completion: &watch::Sender<Option<SharedGenerationResult>>,
) {
    let output = GenerationOutput {
        snapshot: events.snapshot(),
        outcome: GenerationOutcome::Aborted,
        finish_reason: None,
    };
    events.send_item(Ok(GenerationEvent::Finish {
        outcome: GenerationOutcome::Aborted,
        finish_reason: None,
    }));
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

struct GenerationAccumulator {
    model: ModelDescriptor,
    contents: BTreeMap<usize, ContentState>,
    next_content_index: usize,
    usage: Option<Usage>,
    warnings: Vec<Warning>,
    provider_metadata: Extensions,
    finished: bool,
    #[cfg(test)]
    snapshot_materializations: std::sync::atomic::AtomicUsize,
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
            #[cfg(test)]
            snapshot_materializations: std::sync::atomic::AtomicUsize::new(0),
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
        #[cfg(test)]
        self.snapshot_materializations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct MeasurementAllocator;

    static MEASURE_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
    static ALLOCATION_CALLS: AtomicUsize = AtomicUsize::new(0);
    static ALLOCATION_BYTES: AtomicUsize = AtomicUsize::new(0);

    unsafe impl GlobalAlloc for MeasurementAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            if MEASURE_ALLOCATIONS.load(Ordering::Relaxed) {
                ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
                ALLOCATION_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            }
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) };
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc_zeroed(layout) };
            if MEASURE_ALLOCATIONS.load(Ordering::Relaxed) {
                ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
                ALLOCATION_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            }
            pointer
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let pointer = unsafe { System.realloc(pointer, layout, new_size) };
            if MEASURE_ALLOCATIONS.load(Ordering::Relaxed) {
                ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
                ALLOCATION_BYTES.fetch_add(new_size, Ordering::Relaxed);
            }
            pointer
        }
    }

    #[global_allocator]
    static MEASUREMENT_ALLOCATOR: MeasurementAllocator = MeasurementAllocator;

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
        let (events, mut receiver) = event_channel(descriptor());
        events.send_item(Ok(GenerationEvent::Started {
            model: descriptor(),
        }));
        assert!(
            events
                .accept(LanguageAdapterEvent::TextStart { content_index: 0 })
                .expect("text start")
                .is_none()
        );
        for _ in 0..400 {
            assert!(
                events
                    .accept(LanguageAdapterEvent::TextDelta {
                        content_index: 0,
                        delta: "x".repeat(1024),
                    })
                    .expect("text delta")
                    .is_none()
            );
        }
        assert!(
            events
                .accept(LanguageAdapterEvent::TextEnd { content_index: 0 })
                .expect("text end")
                .is_none()
        );
        let output = events
            .accept(LanguageAdapterEvent::Finish {
                finish_reason: None,
            })
            .expect("finish")
            .expect("finished output");

        {
            let state = events
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            assert!(state.pending.len() <= MAX_PENDING_GENERATION_EVENTS);
            assert!(state.non_snapshot_bytes <= MAX_PENDING_GENERATION_BYTES);
        }
        assert_eq!(
            events
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .accumulator
                .snapshot_materializations
                .load(Ordering::Relaxed),
            1,
            "only terminal settlement may materialize before consumer drain"
        );
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
        assert_eq!(snapshot.as_ref(), &output.snapshot);
        assert_eq!(
            receiver
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .accumulator
                .snapshot_materializations
                .load(Ordering::Relaxed),
            2,
            "one resync plus terminal settlement"
        );
    }

    #[test]
    #[ignore = "manual modification-before/after scaling measurement"]
    fn generation_queue_scaling_measurement() {
        for mebibytes in [1usize, 2, 4] {
            let (events, _receiver) = event_channel(descriptor());
            assert!(
                events
                    .accept(LanguageAdapterEvent::TextStart { content_index: 0 })
                    .expect("text start")
                    .is_none()
            );
            let chunk = "x".repeat(16 * 1024);
            let chunks = mebibytes * 1024 * 1024 / chunk.len();
            ALLOCATION_CALLS.store(0, Ordering::Relaxed);
            ALLOCATION_BYTES.store(0, Ordering::Relaxed);
            MEASURE_ALLOCATIONS.store(true, Ordering::Relaxed);
            let started = std::time::Instant::now();
            for _ in 0..chunks {
                assert!(
                    events
                        .accept(LanguageAdapterEvent::TextDelta {
                            content_index: 0,
                            delta: chunk.clone(),
                        })
                        .expect("text delta")
                        .is_none()
                );
            }
            let elapsed = started.elapsed();
            MEASURE_ALLOCATIONS.store(false, Ordering::Relaxed);
            let marker_count = events
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .pending
                .iter()
                .filter(|event| matches!(event, QueuedEvent::ResyncMarker { .. }))
                .count();
            eprintln!(
                "generation_queue_scaling mib={mebibytes} chunks={chunks} queued_resync_markers={marker_count} producer_snapshot_materializations=0 elapsed_us={} allocation_calls={} allocation_bytes={}",
                elapsed.as_micros(),
                ALLOCATION_CALLS.load(Ordering::Relaxed),
                ALLOCATION_BYTES.load(Ordering::Relaxed)
            );
        }
    }
}
