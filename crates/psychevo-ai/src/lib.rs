pub mod adapter;
#[cfg(feature = "anthropic")]
pub mod anthropic;
#[cfg(feature = "openai")]
pub mod builtin_language;
#[cfg(any(feature = "openai", feature = "xiaomi"))]
pub mod builtin_media;
pub mod control;
pub mod credentials;
#[cfg(any(feature = "openai", feature = "anthropic", feature = "xiaomi"))]
pub mod facades;
pub mod fake_sdk;
pub mod generation;
pub mod media;
pub mod provider;
pub mod realtime;
pub mod registry;
pub mod sdk_error;
pub mod sdk_types;
pub mod types;

#[cfg(feature = "openai")]
mod metadata;
#[cfg(feature = "openai")]
mod openai;
#[cfg(any(feature = "openai", feature = "anthropic"))]
mod sse_line;
#[cfg(feature = "openai")]
mod stream;
#[cfg(feature = "xiaomi")]
mod voice;

pub use adapter::{
    AdapterCall, AdapterContext, AdapterFuture, AdapterResult, AdapterStream, GenerationEvent,
    ImageAdapter, LanguageAdapter, LanguageAdapterEvent, RealtimeAdapter, RealtimeAdapterEvent,
    RealtimeAdapterTransport, RealtimeCommandSink, SpeechAdapter, TimeoutPolicy,
    TranscriptionAdapter,
};
#[cfg(feature = "anthropic")]
pub use anthropic::{AnthropicAuth, AnthropicMessagesAdapter};
#[cfg(feature = "openai")]
pub use builtin_language::{OpenAiChatAdapter, OpenAiResponsesAdapter, preview_request};
#[cfg(feature = "openai")]
pub use builtin_media::OpenAiImageAdapter;
#[cfg(feature = "xiaomi")]
pub use builtin_media::{XiaomiSpeechAdapter, XiaomiTranscriptionAdapter};
pub use control::{AbortHandle, AbortSignal};
pub use credentials::{
    CredentialBindings, CredentialRef, CredentialRequest, CredentialResolver, CredentialSlot,
    CredentialSnapshot, EmptyCredentialResolver, EnvironmentCredentialResolver, SecretValue,
    StaticCredentialResolver,
};
#[cfg(feature = "anthropic")]
pub use facades::{Anthropic, AnthropicBuilder};
#[cfg(feature = "openai")]
pub use facades::{OpenAi, OpenAiBuilder};
#[cfg(feature = "xiaomi")]
pub use facades::{Xiaomi, XiaomiBuilder};
pub use fake_sdk::{
    DEFAULT_FAKE_IMAGE_BASE64, Fake, FakeImageAdapter, FakeLanguageAdapter, FakeRealtimeAdapter,
    FakeSpeechAdapter, FakeTranscriptionAdapter,
};
pub use generation::{CompletionHandle, Generation, GenerationError, SharedGenerationResult};
pub use media::{Media, MediaError, MediaInput};
pub use provider::{
    DeploymentConfig, ImageModel, Invocation, LanguageModel, Provider, ProviderBuilder,
    ProviderRuntime, RealtimeModel, SpeechModel, TranscriptionModel,
};
pub use realtime::{RealtimeSender, RealtimeSession};
pub use registry::{Registry, RegistryBuilder};
pub use sdk_error::{ErrorKind, ErrorPhase, ProviderError, SAFE_ERROR_SUMMARY_LIMIT};
pub use sdk_types::{
    AssistantContent, AssistantMessage, Capability, Extensions, FinishReason, FinishReasonKind,
    GeneratedImage, GenerationOutcome, GenerationOutput, GenerationSnapshot, ImageAdapterOutput,
    ImageContent, ImageOutput, ImageRequest, LanguageRequest, LanguageSettings, LanguageTool,
    Message, ModelDescriptor, ModelProfile, ProviderReportedCost, ProviderTool,
    RealtimeCloseReason, RealtimeCommand, RealtimeConnectRequest, RealtimeEvent, RequestHeaders,
    RequestPreview, ResponseFormat, SpeechAdapterOutput, SpeechOutput, SpeechRequest, TextContent,
    ToolArgumentError, ToolArgumentErrorKind, ToolCall, ToolChoice, TranscriptionAdapterOutput,
    TranscriptionOutput, TranscriptionRequest, TranscriptionSegment, Usage, UserContent, Warning,
    WebSearchTool,
};
pub use types::{
    AssistantSource, ImageSearchSource, Outcome, ToolDeclaration, ToolName, UrlCitationSource,
};

#[cfg(feature = "openai")]
pub use openai::http::DEFAULT_INFERENCE_IDLE_TIMEOUT_SECS;

#[cfg(any(feature = "openai", feature = "xiaomi"))]
pub(crate) type Result<T> = std::result::Result<T, types::Error>;
#[cfg(feature = "openai")]
pub(crate) type GenerationStream = futures::stream::BoxStream<'static, Result<types::StreamEvent>>;

#[cfg(all(test, feature = "openai"))]
mod tests;
