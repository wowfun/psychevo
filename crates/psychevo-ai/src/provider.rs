use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::{
    AbortHandle, AbortSignal, AdapterCall, AdapterContext, Capability, CredentialBindings,
    CredentialRequest, CredentialResolver, EmptyCredentialResolver, ErrorKind, ErrorPhase,
    Generation, GenerationError, GenerationOutput, ImageAdapter, ImageOutput, ImageRequest,
    LanguageAdapter, LanguageRequest, ModelDescriptor, ModelProfile, ProviderError,
    RealtimeAdapter, RealtimeConnectRequest, RequestHeaders, SpeechAdapter, SpeechOutput,
    SpeechRequest, TimeoutPolicy, TranscriptionAdapter, TranscriptionOutput, TranscriptionRequest,
    Warning, abort_pair,
};
use crate::{
    ImageAdapterOutput, LanguageInvocationTarget, SpeechAdapterOutput, TranscriptionAdapterOutput,
    start_generation,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentConfig {
    pub deployment_id: String,
    pub provider_family: String,
    pub endpoint: String,
    pub default_language_protocol: String,
    #[serde(default)]
    pub headers: RequestHeaders,
    #[serde(default)]
    pub credentials: CredentialBindings,
    #[serde(default)]
    pub timeout_policy: TimeoutPolicy,
}

impl DeploymentConfig {
    pub fn new(
        deployment_id: impl Into<String>,
        provider_family: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        let provider_family = provider_family.into();
        Self {
            deployment_id: deployment_id.into(),
            default_language_protocol: provider_family.clone(),
            provider_family,
            endpoint: endpoint.into(),
            headers: RequestHeaders::new(),
            credentials: CredentialBindings::default(),
            timeout_policy: TimeoutPolicy::default(),
        }
    }

    pub fn with_default_language_protocol(mut self, protocol: impl Into<String>) -> Self {
        self.default_language_protocol = protocol.into();
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    pub fn with_credentials(mut self, credentials: CredentialBindings) -> Self {
        self.credentials = credentials;
        self
    }

    pub fn with_timeout_policy(mut self, timeout_policy: TimeoutPolicy) -> Self {
        self.timeout_policy = timeout_policy;
        self
    }
}

#[derive(Clone)]
pub struct ProviderRuntime {
    pub(crate) client: reqwest::Client,
    pub(crate) resolver: Arc<dyn CredentialResolver>,
}

impl std::fmt::Debug for ProviderRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProviderRuntime { .. }")
    }
}

impl Default for ProviderRuntime {
    fn default() -> Self {
        Self {
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("default psychevo-ai HTTP client"),
            resolver: Arc::new(EmptyCredentialResolver),
        }
    }
}

impl ProviderRuntime {
    pub fn new(client: reqwest::Client, resolver: Arc<dyn CredentialResolver>) -> Self {
        Self { client, resolver }
    }

    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    pub fn with_credential_resolver(mut self, resolver: Arc<dyn CredentialResolver>) -> Self {
        self.resolver = resolver;
        self
    }
}

#[derive(Clone)]
pub struct Provider {
    config: DeploymentConfig,
    runtime: ProviderRuntime,
    language: Option<Arc<dyn LanguageAdapter>>,
    image: Option<Arc<dyn ImageAdapter>>,
    transcription: Option<Arc<dyn TranscriptionAdapter>>,
    speech: Option<Arc<dyn SpeechAdapter>>,
    realtime: Option<Arc<dyn RealtimeAdapter>>,
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Provider")
            .field("deployment_id", &self.config.deployment_id)
            .field("provider_family", &self.config.provider_family)
            .field("language", &self.language.is_some())
            .field("image", &self.image.is_some())
            .field("transcription", &self.transcription.is_some())
            .field("speech", &self.speech.is_some())
            .field("realtime", &self.realtime.is_some())
            .finish()
    }
}

impl Provider {
    pub fn builder(config: DeploymentConfig) -> ProviderBuilder {
        ProviderBuilder {
            config,
            runtime: ProviderRuntime::default(),
            language: None,
            image: None,
            transcription: None,
            speech: None,
            realtime: None,
        }
    }

    pub fn deployment_config(&self) -> &DeploymentConfig {
        &self.config
    }

    pub fn map_language_adapter(
        mut self,
        wrap: impl FnOnce(Arc<dyn LanguageAdapter>) -> Arc<dyn LanguageAdapter>,
    ) -> Result<Self, ProviderError> {
        let adapter = self
            .language
            .take()
            .ok_or_else(|| unavailable(&self.config.deployment_id, Capability::Language))?;
        self.language = Some(wrap(adapter));
        Ok(self)
    }

    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<LanguageModel, ProviderError> {
        let model_id = validated_model_id(model_id.into())?;
        let adapter = self
            .language
            .clone()
            .ok_or_else(|| unavailable(&self.config.deployment_id, Capability::Language))?;
        Ok(LanguageModel {
            target: LanguageInvocationTarget {
                descriptor: descriptor(
                    &self.config,
                    Capability::Language,
                    model_id,
                    &self.config.default_language_protocol,
                ),
                profile: None,
                endpoint: self.config.endpoint.clone(),
                deployment_headers: self.config.headers.clone(),
                credentials: self.config.credentials.clone(),
                client: self.runtime.client.clone(),
                resolver: self.runtime.resolver.clone(),
                timeout_policy: self.config.timeout_policy.clone(),
                adapter,
            },
        })
    }

    pub fn image_model(&self, model_id: impl Into<String>) -> Result<ImageModel, ProviderError> {
        let model_id = validated_model_id(model_id.into())?;
        let adapter = self
            .image
            .clone()
            .ok_or_else(|| unavailable(&self.config.deployment_id, Capability::Image))?;
        Ok(ImageModel {
            target: UnaryInvocationTarget::new(
                &self.config,
                &self.runtime,
                Capability::Image,
                model_id,
                "image",
            ),
            adapter,
        })
    }

    pub fn transcription_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<TranscriptionModel, ProviderError> {
        let model_id = validated_model_id(model_id.into())?;
        let adapter = self
            .transcription
            .clone()
            .ok_or_else(|| unavailable(&self.config.deployment_id, Capability::Transcription))?;
        Ok(TranscriptionModel {
            target: UnaryInvocationTarget::new(
                &self.config,
                &self.runtime,
                Capability::Transcription,
                model_id,
                "transcription",
            ),
            adapter,
        })
    }

    pub fn speech_model(&self, model_id: impl Into<String>) -> Result<SpeechModel, ProviderError> {
        let model_id = validated_model_id(model_id.into())?;
        let adapter = self
            .speech
            .clone()
            .ok_or_else(|| unavailable(&self.config.deployment_id, Capability::Speech))?;
        Ok(SpeechModel {
            target: UnaryInvocationTarget::new(
                &self.config,
                &self.runtime,
                Capability::Speech,
                model_id,
                "speech",
            ),
            adapter,
        })
    }

    pub fn realtime_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<RealtimeModel, ProviderError> {
        let model_id = validated_model_id(model_id.into())?;
        let adapter = self
            .realtime
            .clone()
            .ok_or_else(|| unavailable(&self.config.deployment_id, Capability::Realtime))?;
        Ok(RealtimeModel {
            target: UnaryInvocationTarget::new(
                &self.config,
                &self.runtime,
                Capability::Realtime,
                model_id,
                "realtime",
            ),
            adapter,
        })
    }
}

pub struct ProviderBuilder {
    config: DeploymentConfig,
    runtime: ProviderRuntime,
    language: Option<Arc<dyn LanguageAdapter>>,
    image: Option<Arc<dyn ImageAdapter>>,
    transcription: Option<Arc<dyn TranscriptionAdapter>>,
    speech: Option<Arc<dyn SpeechAdapter>>,
    realtime: Option<Arc<dyn RealtimeAdapter>>,
}

impl ProviderBuilder {
    pub fn runtime(mut self, runtime: ProviderRuntime) -> Self {
        self.runtime = runtime;
        self
    }

    pub fn credential_resolver(mut self, resolver: Arc<dyn CredentialResolver>) -> Self {
        self.runtime.resolver = resolver;
        self
    }

    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.runtime.client = client;
        self
    }

    pub fn language_adapter(mut self, adapter: impl LanguageAdapter) -> Self {
        self.language = Some(Arc::new(adapter));
        self
    }

    pub fn language_adapter_arc(mut self, adapter: Arc<dyn LanguageAdapter>) -> Self {
        self.language = Some(adapter);
        self
    }

    pub fn image_adapter(mut self, adapter: impl ImageAdapter) -> Self {
        self.image = Some(Arc::new(adapter));
        self
    }

    pub fn image_adapter_arc(mut self, adapter: Arc<dyn ImageAdapter>) -> Self {
        self.image = Some(adapter);
        self
    }

    pub fn transcription_adapter(mut self, adapter: impl TranscriptionAdapter) -> Self {
        self.transcription = Some(Arc::new(adapter));
        self
    }

    pub fn transcription_adapter_arc(mut self, adapter: Arc<dyn TranscriptionAdapter>) -> Self {
        self.transcription = Some(adapter);
        self
    }

    pub fn speech_adapter(mut self, adapter: impl SpeechAdapter) -> Self {
        self.speech = Some(Arc::new(adapter));
        self
    }

    pub fn speech_adapter_arc(mut self, adapter: Arc<dyn SpeechAdapter>) -> Self {
        self.speech = Some(adapter);
        self
    }

    pub fn realtime_adapter(mut self, adapter: impl RealtimeAdapter) -> Self {
        self.realtime = Some(Arc::new(adapter));
        self
    }

    pub fn realtime_adapter_arc(mut self, adapter: Arc<dyn RealtimeAdapter>) -> Self {
        self.realtime = Some(adapter);
        self
    }

    pub fn build(self) -> Result<Provider, ProviderError> {
        validate_deployment_config(&self.config)?;
        Ok(Provider {
            config: self.config,
            runtime: self.runtime,
            language: self.language,
            image: self.image,
            transcription: self.transcription,
            speech: self.speech,
            realtime: self.realtime,
        })
    }
}

#[derive(Clone)]
pub struct LanguageModel {
    target: LanguageInvocationTarget,
}

impl std::fmt::Debug for LanguageModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LanguageModel")
            .field("descriptor", &self.target.descriptor)
            .finish_non_exhaustive()
    }
}

impl LanguageModel {
    pub fn descriptor(&self) -> &ModelDescriptor {
        &self.target.descriptor
    }

    pub fn with_profile(mut self, profile: ModelProfile) -> Self {
        self.target.profile = Some(profile);
        self
    }

    pub fn profile(&self) -> Option<&ModelProfile> {
        self.target.profile.as_ref()
    }

    pub fn stream(&self, request: LanguageRequest) -> Generation {
        start_generation(self.target.clone(), request)
    }

    pub fn generate(
        &self,
        request: LanguageRequest,
    ) -> Invocation<GenerationOutput, GenerationError> {
        let generation = self.stream(request);
        let abort = generation.abort_handle();
        Invocation::from_future(abort, async move { generation.finish().await })
    }
}

#[derive(Clone)]
pub struct ImageModel {
    target: UnaryInvocationTarget,
    adapter: Arc<dyn ImageAdapter>,
}

impl ImageModel {
    pub fn descriptor(&self) -> &ModelDescriptor {
        &self.target.descriptor
    }

    pub fn with_profile(mut self, profile: ModelProfile) -> Self {
        self.target.profile = Some(profile);
        self
    }

    pub fn profile(&self) -> Option<&ModelProfile> {
        self.target.profile.as_ref()
    }

    pub fn generate(&self, request: ImageRequest) -> Invocation<ImageOutput> {
        let target = self.target.clone();
        let adapter = self.adapter.clone();
        let requested_count = request.count;
        spawn_invocation(move |abort| async move {
            let total_deadline = invocation_total_deadline(&target);
            if requested_count == 0 {
                return Err(ProviderError::invalid_request(
                    "image count must be at least one",
                ));
            }
            let context =
                prepare_adapter_context(&target, &request.headers, abort.clone(), total_deadline)
                    .await?;
            let output = guarded_adapter_call(
                adapter.generate(AdapterCall {
                    model: target.descriptor.model_id.clone(),
                    request,
                    context,
                }),
                abort,
                &target,
                ErrorPhase::ResponseBody,
                total_deadline,
            )
            .await?;
            normalize_image_output(&target.descriptor, requested_count, output)
        })
    }
}

#[derive(Clone)]
pub struct TranscriptionModel {
    target: UnaryInvocationTarget,
    adapter: Arc<dyn TranscriptionAdapter>,
}

impl TranscriptionModel {
    pub fn descriptor(&self) -> &ModelDescriptor {
        &self.target.descriptor
    }

    pub fn with_profile(mut self, profile: ModelProfile) -> Self {
        self.target.profile = Some(profile);
        self
    }

    pub fn profile(&self) -> Option<&ModelProfile> {
        self.target.profile.as_ref()
    }

    pub fn transcribe(&self, request: TranscriptionRequest) -> Invocation<TranscriptionOutput> {
        let target = self.target.clone();
        let adapter = self.adapter.clone();
        spawn_invocation(move |abort| async move {
            let total_deadline = invocation_total_deadline(&target);
            let context =
                prepare_adapter_context(&target, &request.headers, abort.clone(), total_deadline)
                    .await?;
            let output = guarded_adapter_call(
                adapter.transcribe(AdapterCall {
                    model: target.descriptor.model_id.clone(),
                    request,
                    context,
                }),
                abort,
                &target,
                ErrorPhase::ResponseBody,
                total_deadline,
            )
            .await?;
            normalize_transcription_output(&target.descriptor, output)
        })
    }
}

#[derive(Clone)]
pub struct SpeechModel {
    target: UnaryInvocationTarget,
    adapter: Arc<dyn SpeechAdapter>,
}

impl SpeechModel {
    pub fn descriptor(&self) -> &ModelDescriptor {
        &self.target.descriptor
    }

    pub fn with_profile(mut self, profile: ModelProfile) -> Self {
        self.target.profile = Some(profile);
        self
    }

    pub fn profile(&self) -> Option<&ModelProfile> {
        self.target.profile.as_ref()
    }

    pub fn synthesize(&self, request: SpeechRequest) -> Invocation<SpeechOutput> {
        let target = self.target.clone();
        let adapter = self.adapter.clone();
        spawn_invocation(move |abort| async move {
            let total_deadline = invocation_total_deadline(&target);
            let context =
                prepare_adapter_context(&target, &request.headers, abort.clone(), total_deadline)
                    .await?;
            let output = guarded_adapter_call(
                adapter.synthesize(AdapterCall {
                    model: target.descriptor.model_id.clone(),
                    request,
                    context,
                }),
                abort,
                &target,
                ErrorPhase::ResponseBody,
                total_deadline,
            )
            .await?;
            normalize_speech_output(&target.descriptor, output)
        })
    }
}

#[derive(Clone)]
pub struct RealtimeModel {
    pub(crate) target: UnaryInvocationTarget,
    pub(crate) adapter: Arc<dyn RealtimeAdapter>,
}

impl RealtimeModel {
    pub fn descriptor(&self) -> &ModelDescriptor {
        &self.target.descriptor
    }

    pub fn with_profile(mut self, profile: ModelProfile) -> Self {
        self.target.profile = Some(profile);
        self
    }

    pub fn profile(&self) -> Option<&ModelProfile> {
        self.target.profile.as_ref()
    }

    pub fn connect(&self, request: RealtimeConnectRequest) -> Invocation<crate::RealtimeSession> {
        crate::start_realtime_connect(self.target.clone(), self.adapter.clone(), request)
    }
}

pub struct Invocation<T, E = ProviderError> {
    future: Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'static>>,
    abort: AbortHandle,
    owns_invocation: bool,
}

impl<T, E> std::fmt::Debug for Invocation<T, E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Invocation")
            .field("aborted", &self.abort.is_aborted())
            .finish_non_exhaustive()
    }
}

impl<T, E> Invocation<T, E> {
    pub(crate) fn from_future(
        abort: AbortHandle,
        future: impl Future<Output = Result<T, E>> + Send + 'static,
    ) -> Self {
        Self {
            future: Box::pin(future),
            abort,
            owns_invocation: true,
        }
    }

    pub fn abort(&self) -> bool {
        self.abort.abort()
    }

    pub fn abort_handle(&self) -> AbortHandle {
        self.abort.clone()
    }
}

impl<T, E> Future for Invocation<T, E> {
    type Output = Result<T, E>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        match self.future.as_mut().poll(context) {
            Poll::Ready(output) => {
                self.owns_invocation = false;
                Poll::Ready(output)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T, E> Drop for Invocation<T, E> {
    fn drop(&mut self) {
        if self.owns_invocation {
            self.abort.abort();
        }
    }
}

pub(crate) fn spawn_invocation<T, F, Fut>(operation: F) -> Invocation<T>
where
    T: Send + 'static,
    F: FnOnce(AbortSignal) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, ProviderError>> + Send + 'static,
{
    spawn_invocation_with_pair(move |_abort, signal| operation(signal))
}

pub(crate) fn spawn_invocation_with_pair<T, F, Fut>(operation: F) -> Invocation<T>
where
    T: Send + 'static,
    F: FnOnce(AbortHandle, AbortSignal) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, ProviderError>> + Send + 'static,
{
    let (abort, signal) = abort_pair();
    let operation_abort = abort.clone();
    let (sender, receiver) = oneshot::channel();
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(async move {
                let _ = sender.send(operation(operation_abort, signal).await);
            });
            Invocation::from_future(abort, async move {
                receiver.await.unwrap_or_else(|_| {
                    Err(ProviderError::new(
                        ErrorKind::Protocol,
                        ErrorPhase::Runtime,
                        "invocation task ended without a result",
                    ))
                })
            })
        }
        Err(_) => {
            Invocation::from_future(abort, async { Err(ProviderError::runtime_unavailable()) })
        }
    }
}

#[derive(Clone)]
pub(crate) struct UnaryInvocationTarget {
    pub descriptor: ModelDescriptor,
    pub profile: Option<ModelProfile>,
    pub endpoint: String,
    pub deployment_headers: RequestHeaders,
    pub credentials: CredentialBindings,
    pub client: reqwest::Client,
    pub resolver: Arc<dyn CredentialResolver>,
    pub timeout_policy: TimeoutPolicy,
}

impl UnaryInvocationTarget {
    fn new(
        config: &DeploymentConfig,
        runtime: &ProviderRuntime,
        capability: Capability,
        model_id: String,
        protocol_id: &str,
    ) -> Self {
        Self {
            descriptor: descriptor(config, capability, model_id, protocol_id),
            profile: None,
            endpoint: config.endpoint.clone(),
            deployment_headers: config.headers.clone(),
            credentials: config.credentials.clone(),
            client: runtime.client.clone(),
            resolver: runtime.resolver.clone(),
            timeout_policy: config.timeout_policy.clone(),
        }
    }
}

pub(crate) async fn prepare_adapter_context(
    target: &UnaryInvocationTarget,
    invocation_headers: &RequestHeaders,
    abort: AbortSignal,
    total_deadline: Option<tokio::time::Instant>,
) -> Result<AdapterContext, ProviderError> {
    let headers = crate::merge_safe_headers(&target.deployment_headers, invocation_headers)?;
    let credentials = guarded_adapter_call(
        target.resolver.resolve(CredentialRequest {
            deployment_id: target.descriptor.deployment_id.clone(),
            provider_family: target.descriptor.provider_family.clone(),
            bindings: target.credentials.clone(),
        }),
        abort.clone(),
        target,
        ErrorPhase::Credentials,
        total_deadline,
    )
    .await?;
    Ok(AdapterContext {
        model: target.descriptor.clone(),
        profile: target.profile.clone(),
        endpoint: target.endpoint.clone(),
        headers,
        client: target.client.clone(),
        credentials,
        abort,
        timeout_policy: target.timeout_policy.clone(),
    })
}

pub(crate) async fn guarded_adapter_call<T>(
    future: impl Future<Output = Result<T, ProviderError>>,
    mut abort: AbortSignal,
    target: &UnaryInvocationTarget,
    phase: ErrorPhase,
    total_deadline: Option<tokio::time::Instant>,
) -> Result<T, ProviderError> {
    let idle = target.timeout_policy.progress_idle_timeout();
    tokio::pin!(future);
    tokio::select! {
        biased;
        _ = abort.wait_for_abort() => Err(ProviderError::aborted(phase)),
        _ = wait_for_deadline(total_deadline) => Err(ProviderError::new(
            ErrorKind::Timeout,
            phase,
            "invocation total deadline elapsed",
        )),
        _ = wait_for_duration(idle) => Err(ProviderError::new(
            ErrorKind::Timeout,
            phase,
            "provider made no invocation progress",
        )),
        result = &mut future => result,
    }
}

pub(crate) fn invocation_total_deadline(
    target: &UnaryInvocationTarget,
) -> Option<tokio::time::Instant> {
    target
        .timeout_policy
        .total_deadline()
        .map(|duration| tokio::time::Instant::now() + duration)
}

async fn wait_for_duration(duration: Option<std::time::Duration>) {
    match duration {
        Some(duration) => tokio::time::sleep(duration).await,
        None => std::future::pending().await,
    }
}

async fn wait_for_deadline(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

fn normalize_image_output(
    descriptor: &ModelDescriptor,
    requested_count: u32,
    mut output: ImageAdapterOutput,
) -> Result<ImageOutput, ProviderError> {
    if output.images.is_empty() {
        return Err(ProviderError::provider(
            ErrorPhase::ResponseBody,
            None,
            None,
            "image provider returned no images",
        ));
    }
    for image in &output.images {
        validate_output_media(&image.media, "image")?;
    }
    if output.images.len() < requested_count as usize {
        output.warnings.push(Warning::new(
            "fewer_images_than_requested",
            format!(
                "provider returned {} of {requested_count} requested images",
                output.images.len()
            ),
        ));
    }
    Ok(ImageOutput {
        model: descriptor.clone(),
        images: output.images,
        usage: output.usage,
        warnings: output.warnings,
        provider_metadata: output.provider_metadata,
    })
}

fn normalize_transcription_output(
    descriptor: &ModelDescriptor,
    output: TranscriptionAdapterOutput,
) -> Result<TranscriptionOutput, ProviderError> {
    let mut previous_start = None;
    for segment in &output.segments {
        if !segment.start_seconds.is_finite()
            || !segment.end_seconds.is_finite()
            || segment.start_seconds < 0.0
            || segment.end_seconds < segment.start_seconds
        {
            return Err(ProviderError::protocol(
                "transcription segment times must be finite, non-negative, and ordered",
            ));
        }
        if previous_start.is_some_and(|start| segment.start_seconds < start) {
            return Err(ProviderError::protocol(
                "transcription segments must be ordered by start time",
            ));
        }
        previous_start = Some(segment.start_seconds);
    }
    if output
        .duration_seconds
        .is_some_and(|duration| !duration.is_finite() || duration < 0.0)
    {
        return Err(ProviderError::protocol(
            "transcription duration must be finite and non-negative",
        ));
    }
    Ok(TranscriptionOutput {
        model: descriptor.clone(),
        text: output.text,
        segments: output.segments,
        language: output.language,
        duration_seconds: output.duration_seconds,
        warnings: output.warnings,
        provider_metadata: output.provider_metadata,
    })
}

fn normalize_speech_output(
    descriptor: &ModelDescriptor,
    output: SpeechAdapterOutput,
) -> Result<SpeechOutput, ProviderError> {
    validate_output_media(&output.audio, "speech")?;
    Ok(SpeechOutput {
        model: descriptor.clone(),
        audio: output.audio,
        warnings: output.warnings,
        provider_metadata: output.provider_metadata,
    })
}

fn validate_output_media(media: &crate::Media, context: &str) -> Result<(), ProviderError> {
    if media.mime_type().trim().is_empty() || media.mime_type().chars().any(char::is_control) {
        return Err(ProviderError::new(
            ErrorKind::Protocol,
            ErrorPhase::ResponseBody,
            format!("{context} provider returned media without a valid MIME type"),
        ));
    }
    let bytes = media.bytes().map_err(|error| {
        ProviderError::new(
            ErrorKind::Protocol,
            ErrorPhase::ResponseBody,
            format!("{context} provider returned invalid media: {error}"),
        )
    })?;
    if bytes.is_empty() {
        return Err(ProviderError::new(
            ErrorKind::Protocol,
            ErrorPhase::ResponseBody,
            format!("{context} provider returned empty media"),
        ));
    }
    Ok(())
}

fn descriptor(
    config: &DeploymentConfig,
    capability: Capability,
    model_id: String,
    protocol_id: &str,
) -> ModelDescriptor {
    ModelDescriptor {
        deployment_id: config.deployment_id.clone(),
        provider_family: config.provider_family.clone(),
        capability,
        model_id,
        protocol_id: protocol_id.to_string(),
    }
}

fn unavailable(deployment_id: &str, capability: Capability) -> ProviderError {
    ProviderError::configuration(format!(
        "deployment `{deployment_id}` does not provide {capability:?}"
    ))
}

fn validated_model_id(model_id: String) -> Result<String, ProviderError> {
    if model_id.is_empty() || model_id.trim() != model_id || model_id.chars().any(char::is_control)
    {
        return Err(ProviderError::configuration(
            "model id must be non-empty, trimmed, and contain no control characters",
        ));
    }
    Ok(model_id)
}

fn validate_deployment_config(config: &DeploymentConfig) -> Result<(), ProviderError> {
    if !valid_deployment_id(&config.deployment_id) {
        return Err(ProviderError::configuration(format!(
            "invalid deployment id `{}`; expected [a-z0-9][a-z0-9_-]*",
            config.deployment_id
        )));
    }
    if config.provider_family.trim().is_empty() {
        return Err(ProviderError::configuration(
            "provider family must not be empty",
        ));
    }
    if config.endpoint.trim().is_empty() {
        return Err(ProviderError::configuration(
            "provider endpoint must not be empty",
        ));
    }
    let endpoint = reqwest::Url::parse(&config.endpoint).map_err(|error| {
        ProviderError::configuration(format!("invalid provider endpoint: {error}"))
    })?;
    if !endpoint.username().is_empty() || endpoint.password().is_some() {
        return Err(ProviderError::configuration(
            "provider endpoint must not contain userinfo",
        ));
    }
    if config.default_language_protocol.trim().is_empty() {
        return Err(ProviderError::configuration(
            "default language protocol must not be empty",
        ));
    }
    crate::merge_safe_headers(&RequestHeaders::new(), &config.headers)?;
    for (slot, credential_ref) in &config.credentials.0 {
        if slot.0.trim().is_empty() || credential_ref.0.trim().is_empty() {
            return Err(ProviderError::configuration(
                "credential slots and references must not be empty",
            ));
        }
    }
    Ok(())
}

fn valid_deployment_id(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}
