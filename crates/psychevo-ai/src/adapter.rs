use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AbortSignal, AssistantSource, CredentialSnapshot, FinishReason, ImageAdapterOutput,
    ImageRequest, LanguageRequest, ModelDescriptor, ModelProfile, ProviderError, ProviderTool,
    RealtimeCommand, RealtimeConnectRequest, SpeechAdapterOutput, SpeechRequest, ToolArgumentError,
    TranscriptionAdapterOutput, TranscriptionRequest, Usage, Warning,
};

pub type AdapterResult<T> = Result<T, ProviderError>;
pub type AdapterFuture<'a, T> = Pin<Box<dyn Future<Output = AdapterResult<T>> + Send + 'a>>;
pub type AdapterStream<E> = Pin<Box<dyn Stream<Item = AdapterResult<E>> + Send + 'static>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutPolicy {
    pub connect_timeout_secs: u64,
    pub progress_idle_timeout_secs: u64,
    pub total_deadline_secs: u64,
    pub realtime_command_timeout_secs: u64,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            connect_timeout_secs: 10,
            progress_idle_timeout_secs: 300,
            total_deadline_secs: 0,
            realtime_command_timeout_secs: 0,
        }
    }
}

impl TimeoutPolicy {
    pub fn progress_idle_timeout(&self) -> Option<Duration> {
        duration_or_none(self.progress_idle_timeout_secs)
    }

    pub fn total_deadline(&self) -> Option<Duration> {
        duration_or_none(self.total_deadline_secs)
    }

    pub fn realtime_command_timeout(&self) -> Option<Duration> {
        duration_or_none(self.realtime_command_timeout_secs)
    }
}

fn duration_or_none(seconds: u64) -> Option<Duration> {
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

#[derive(Clone)]
pub struct AdapterContext {
    pub model: ModelDescriptor,
    pub profile: Option<ModelProfile>,
    pub endpoint: String,
    pub headers: BTreeMap<String, String>,
    pub client: reqwest::Client,
    pub credentials: CredentialSnapshot,
    pub abort: AbortSignal,
    pub timeout_policy: TimeoutPolicy,
}

impl std::fmt::Debug for AdapterContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdapterContext")
            .field("model", &self.model)
            .field("profile", &self.profile)
            .field("endpoint", &self.endpoint)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("credentials", &self.credentials)
            .field("timeout_policy", &self.timeout_policy)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct AdapterCall<R> {
    pub model: String,
    pub request: R,
    pub context: AdapterContext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LanguageAdapterEvent {
    TextStart {
        content_index: usize,
    },
    TextDelta {
        content_index: usize,
        delta: String,
    },
    TextEnd {
        content_index: usize,
    },
    ReasoningStart {
        content_index: usize,
    },
    ReasoningDelta {
        content_index: usize,
        delta: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_evidence: Option<Value>,
    },
    ReasoningEnd {
        content_index: usize,
    },
    ToolCallStart {
        content_index: usize,
        id: String,
        name: String,
    },
    ToolCallArgumentsDelta {
        content_index: usize,
        delta: String,
    },
    ToolCallEnd {
        content_index: usize,
        arguments_raw: String,
    },
    ProviderToolStart {
        content_index: usize,
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<Value>,
    },
    ProviderToolEnd {
        content_index: usize,
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<Value>,
        status: String,
    },
    Source {
        content_index: usize,
        source: AssistantSource,
    },
    Usage {
        usage: Usage,
    },
    Metadata {
        metadata: BTreeMap<String, Value>,
    },
    Warning {
        warning: Warning,
    },
    Finish {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finish_reason: Option<FinishReason>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GenerationEvent {
    Started {
        model: ModelDescriptor,
    },
    TextStart {
        content_index: usize,
    },
    TextDelta {
        content_index: usize,
        delta: String,
    },
    TextEnd {
        content_index: usize,
    },
    ReasoningStart {
        content_index: usize,
    },
    ReasoningDelta {
        content_index: usize,
        delta: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_evidence: Option<Value>,
    },
    ReasoningEnd {
        content_index: usize,
    },
    ToolCallStart {
        content_index: usize,
        id: String,
        name: String,
    },
    ToolCallArgumentsDelta {
        content_index: usize,
        delta: String,
    },
    ToolCallEnd {
        content_index: usize,
        arguments_raw: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        argument_error: Option<ToolArgumentError>,
    },
    ProviderToolStart {
        content_index: usize,
        tool: ProviderTool,
    },
    ProviderToolEnd {
        content_index: usize,
        tool: ProviderTool,
    },
    Source {
        content_index: usize,
        source: AssistantSource,
    },
    Usage {
        usage: Usage,
    },
    Metadata {
        metadata: BTreeMap<String, Value>,
    },
    Warning {
        warning: Warning,
    },
    Resync {
        snapshot: Box<crate::GenerationSnapshot>,
        dropped_events: u64,
    },
    Finish {
        outcome: crate::GenerationOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finish_reason: Option<FinishReason>,
    },
}

pub trait LanguageAdapter: Send + Sync + 'static {
    fn stream(
        &self,
        call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>>;
}

pub trait ImageAdapter: Send + Sync + 'static {
    fn generate(&self, call: AdapterCall<ImageRequest>) -> AdapterFuture<'_, ImageAdapterOutput>;
}

pub trait TranscriptionAdapter: Send + Sync + 'static {
    fn transcribe(
        &self,
        call: AdapterCall<TranscriptionRequest>,
    ) -> AdapterFuture<'_, TranscriptionAdapterOutput>;
}

pub trait SpeechAdapter: Send + Sync + 'static {
    fn synthesize(
        &self,
        call: AdapterCall<SpeechRequest>,
    ) -> AdapterFuture<'_, SpeechAdapterOutput>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RealtimeAdapterEvent {
    InputTranscriptDelta { delta: String },
    InputTranscriptDone { text: String },
    OutputTextDelta { delta: String },
    OutputTextDone { text: String },
    OutputAudioDelta { audio: crate::Media },
    OutputAudioDone,
    ResponseDone,
    Warning { warning: Warning },
    Metadata { metadata: BTreeMap<String, Value> },
    Closed { remote: bool },
}

pub trait RealtimeCommandSink: Send + Sync + 'static {
    fn send(&self, command: RealtimeCommand) -> AdapterFuture<'_, ()>;
    fn close(&self) -> AdapterFuture<'_, ()>;
}

pub struct RealtimeAdapterTransport {
    pub commands: Arc<dyn RealtimeCommandSink>,
    pub events: AdapterStream<RealtimeAdapterEvent>,
}

pub trait RealtimeAdapter: Send + Sync + 'static {
    fn connect(
        &self,
        call: AdapterCall<RealtimeConnectRequest>,
    ) -> AdapterFuture<'_, RealtimeAdapterTransport>;
}
