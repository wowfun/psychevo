use std::collections::BTreeMap;
#[cfg(any(feature = "openai", feature = "anthropic", feature = "xiaomi"))]
use std::io::{Read, Write};
#[cfg(any(feature = "openai", feature = "anthropic", feature = "xiaomi"))]
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(feature = "openai")]
use std::thread;

use futures::{StreamExt, future::join_all, stream};
#[cfg(feature = "anthropic")]
use psychevo_ai::Anthropic;
#[cfg(feature = "openai")]
use psychevo_ai::OpenAi;
#[cfg(any(feature = "openai", feature = "anthropic", feature = "xiaomi"))]
use psychevo_ai::ProviderRuntime;
#[cfg(feature = "xiaomi")]
use psychevo_ai::Xiaomi;
use psychevo_ai::{
    AdapterCall, AdapterFuture, AdapterStream, AssistantContent, Capability, DeploymentConfig,
    ErrorKind, ErrorPhase, Fake, FakeLanguageAdapter, GeneratedImage, GenerationEvent,
    GenerationOutcome, ImageAdapter, ImageAdapterOutput, ImageRequest, LanguageAdapter,
    LanguageAdapterEvent, LanguageRequest, Media, Message, ModelProfile, Provider, ProviderError,
    Registry, TimeoutPolicy, ToolArgumentErrorKind, TranscriptionAdapter,
    TranscriptionAdapterOutput, TranscriptionRequest, TranscriptionSegment,
};
use psychevo_ai::{
    CredentialBindings, CredentialRequest, CredentialResolver, CredentialSlot, CredentialSnapshot,
    RealtimeAdapter, RealtimeAdapterEvent, RealtimeAdapterTransport, RealtimeCommand,
    RealtimeCommandSink, RealtimeConnectRequest, SecretValue,
};

#[cfg(any(feature = "openai", feature = "anthropic", feature = "xiaomi"))]
fn single_http_response(
    status: &str,
    retry_after_seconds: Option<u64>,
    body: &str,
) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let address = listener.local_addr().expect("address");
    let status = status.to_string();
    let body = body.as_bytes().to_vec();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let mut expected_length = None;
        loop {
            let read = socket.read(&mut buffer).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if expected_length.is_none()
                && let Some(header_end) =
                    request.windows(4).position(|window| window == b"\r\n\r\n")
            {
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or_default();
                expected_length = Some(header_end + 4 + content_length);
            }
            if expected_length.is_some_and(|length| request.len() >= length) {
                break;
            }
        }
        let retry_after = retry_after_seconds
            .map(|seconds| format!("retry-after: {seconds}\r\n"))
            .unwrap_or_default();
        write!(
            socket,
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n{retry_after}content-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        )
        .expect("write headers");
        socket.write_all(&body).expect("write body");
        socket.flush().expect("flush");
    });
    (format!("http://{address}/v1"), server)
}

#[derive(Debug)]
struct ExternalCustomAdapter;

impl LanguageAdapter for ExternalCustomAdapter {
    fn stream(
        &self,
        call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        let model = call.model;
        Box::pin(async move {
            Ok(Box::pin(stream::iter([
                Ok(LanguageAdapterEvent::TextStart { content_index: 0 }),
                Ok(LanguageAdapterEvent::TextDelta {
                    content_index: 0,
                    delta: format!("custom:{model}"),
                }),
                Ok(LanguageAdapterEvent::TextEnd { content_index: 0 }),
                Ok(LanguageAdapterEvent::Finish {
                    finish_reason: None,
                }),
            ])) as AdapterStream<_>)
        })
    }
}

#[tokio::test]
async fn external_adapter_uses_public_provider_and_registry_seams() {
    let provider = Provider::builder(
        DeploymentConfig::new("local", "example", "example://local")
            .with_default_language_protocol("example_stream"),
    )
    .language_adapter(ExternalCustomAdapter)
    .build()
    .expect("provider");
    let registry = Registry::builder()
        .register(provider)
        .expect("register")
        .build();
    let model = registry
        .language_model("local/org/model")
        .expect("bound model");
    assert_eq!(model.descriptor().capability, Capability::Language);
    assert_eq!(model.descriptor().model_id, "org/model");

    let mut generation = model.stream(LanguageRequest {
        messages: vec![Message::user("hello")],
        ..LanguageRequest::default()
    });
    assert!(matches!(
        generation.next().await,
        Some(Ok(GenerationEvent::Started { .. }))
    ));
    let output = generation.finish().await.expect("output");
    assert_eq!(output.outcome, GenerationOutcome::Completed);
    assert!(matches!(
        &output.snapshot.assistant.content[0],
        AssistantContent::Text(content) if content.text == "custom:org/model"
    ));
}

#[derive(Debug)]
struct ProfileObservingAdapter {
    observed: Arc<Mutex<Option<ModelProfile>>>,
}

impl LanguageAdapter for ProfileObservingAdapter {
    fn stream(
        &self,
        call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        *self.observed.lock().expect("profile observation") = call.context.profile;
        Box::pin(async {
            Ok(Box::pin(stream::iter([Ok(LanguageAdapterEvent::Finish {
                finish_reason: None,
            })])) as AdapterStream<_>)
        })
    }
}

#[tokio::test]
async fn bound_model_profile_is_visible_to_custom_adapter() {
    let observed = Arc::new(Mutex::new(None));
    let provider = Provider::builder(DeploymentConfig::new(
        "profiled",
        "example",
        "example://profiled",
    ))
    .language_adapter(ProfileObservingAdapter {
        observed: observed.clone(),
    })
    .build()
    .expect("provider");
    let registry = Registry::builder()
        .register(provider)
        .expect("register")
        .build();
    let profile = ModelProfile {
        capabilities: BTreeMap::from([("image_input".to_string(), false)]),
        metadata: BTreeMap::from([("context_limit".to_string(), serde_json::json!(8192))]),
    };
    let model = registry
        .language_model_with_profile("profiled/model", profile.clone())
        .expect("profiled model");

    assert_eq!(model.profile(), Some(&profile));
    model
        .generate(LanguageRequest::default())
        .await
        .expect("generation");
    assert_eq!(
        observed.lock().expect("profile observation").as_ref(),
        Some(&profile)
    );
}

#[tokio::test]
async fn invalid_final_tool_arguments_are_successful_and_lossless() {
    let fake = Fake::with_language(FakeLanguageAdapter::single(vec![
        LanguageAdapterEvent::ToolCallStart {
            content_index: 0,
            id: "call-1".to_string(),
            name: "read".to_string(),
        },
        LanguageAdapterEvent::ToolCallArgumentsDelta {
            content_index: 0,
            delta: "{\"path\":".to_string(),
        },
        LanguageAdapterEvent::ToolCallEnd {
            content_index: 0,
            arguments_raw: "{\"path\":".to_string(),
        },
        LanguageAdapterEvent::Finish {
            finish_reason: None,
        },
    ]))
    .expect("fake");
    let model = fake.provider().language_model("tool-model").expect("model");
    let output = model
        .generate(LanguageRequest::default())
        .await
        .expect("generation remains successful");
    let AssistantContent::ToolCall(call) = &output.snapshot.assistant.content[0] else {
        panic!("tool call")
    };
    assert_eq!(call.arguments_raw, "{\"path\":");
    assert_eq!(
        call.argument_error.as_ref().map(|error| &error.kind),
        Some(&ToolArgumentErrorKind::InvalidJson)
    );
    assert!(call.arguments.is_none());
}

#[tokio::test]
async fn tool_argument_fragmentation_boundary_matrix_is_lossless() {
    let cases = [
        ("", Some(ToolArgumentErrorKind::InvalidJson)),
        ("{}", None),
        ("[]", Some(ToolArgumentErrorKind::NotAnObject)),
        (r#"{"path":"中.txt"}"#, None),
        (r#"{"path":"#, Some(ToolArgumentErrorKind::InvalidJson)),
    ];

    for (arguments_raw, expected_error) in cases {
        let mut partitions = vec![vec![arguments_raw.to_string()]];
        let boundaries = arguments_raw
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(arguments_raw.len()))
            .filter(|index| *index > 0 && *index < arguments_raw.len())
            .collect::<Vec<_>>();
        for boundary in boundaries {
            partitions.push(vec![
                arguments_raw[..boundary].to_string(),
                arguments_raw[boundary..].to_string(),
            ]);
        }
        if !arguments_raw.is_empty() {
            partitions.push(arguments_raw.chars().map(String::from).collect());
        }

        for fragments in partitions {
            let mut script = vec![LanguageAdapterEvent::ToolCallStart {
                content_index: 0,
                id: "call-1".to_string(),
                name: "read".to_string(),
            }];
            script.extend(fragments.into_iter().map(|delta| {
                LanguageAdapterEvent::ToolCallArgumentsDelta {
                    content_index: 0,
                    delta,
                }
            }));
            script.extend([
                LanguageAdapterEvent::ToolCallEnd {
                    content_index: 0,
                    arguments_raw: arguments_raw.to_string(),
                },
                LanguageAdapterEvent::Finish {
                    finish_reason: None,
                },
            ]);

            let fake = Fake::with_language(FakeLanguageAdapter::single(script)).expect("fake");
            let model = fake
                .provider()
                .language_model("tool-fragmentation")
                .expect("model");
            let mut generation = model.stream(LanguageRequest::default());
            let mut streamed_arguments = String::new();
            let mut tool_terminations = 0;
            let mut generation_terminals = 0;
            while let Some(event) = generation.next_event().await {
                match event.expect("finite well-ordered fragments") {
                    GenerationEvent::ToolCallArgumentsDelta { delta, .. } => {
                        streamed_arguments.push_str(&delta);
                    }
                    GenerationEvent::ToolCallEnd {
                        arguments_raw: terminal_arguments,
                        ..
                    } => {
                        assert_eq!(terminal_arguments, arguments_raw);
                        tool_terminations += 1;
                    }
                    GenerationEvent::Finish { .. } => {
                        generation_terminals += 1;
                        break;
                    }
                    _ => {}
                }
            }
            let output = generation.finish().await.expect("generation output");
            assert_eq!(streamed_arguments, arguments_raw);
            assert_eq!(tool_terminations, 1);
            assert_eq!(generation_terminals, 1);
            assert_eq!(output.outcome, GenerationOutcome::Completed);
            let AssistantContent::ToolCall(call) = &output.snapshot.assistant.content[0] else {
                panic!("tool call")
            };
            assert_eq!(call.arguments_raw, arguments_raw);
            assert_eq!(
                call.argument_error.as_ref().map(|error| &error.kind),
                expected_error.as_ref()
            );
        }
    }
}

#[tokio::test]
async fn premature_adapter_eof_is_protocol_error_with_partial_output() {
    let fake = Fake::with_language(FakeLanguageAdapter::single(vec![
        LanguageAdapterEvent::TextStart { content_index: 0 },
        LanguageAdapterEvent::TextDelta {
            content_index: 0,
            delta: "partial".to_string(),
        },
    ]))
    .expect("fake");
    let model = fake.provider().language_model("eof").expect("model");
    let error = model
        .generate(LanguageRequest::default())
        .await
        .expect_err("premature EOF");
    assert_eq!(error.error.kind, ErrorKind::Protocol);
    assert!(matches!(
        &error.partial.assistant.content[0],
        AssistantContent::Text(content) if content.text == "partial"
    ));
}

#[tokio::test]
async fn invalid_adapter_content_lifecycles_are_protocol_errors() {
    for script in [
        vec![
            LanguageAdapterEvent::TextDelta {
                content_index: 0,
                delta: "missing start".to_string(),
            },
            LanguageAdapterEvent::Finish {
                finish_reason: None,
            },
        ],
        vec![
            LanguageAdapterEvent::TextStart { content_index: 1 },
            LanguageAdapterEvent::Finish {
                finish_reason: None,
            },
        ],
        vec![
            LanguageAdapterEvent::TextStart { content_index: 0 },
            LanguageAdapterEvent::Finish {
                finish_reason: None,
            },
        ],
    ] {
        let fake = Fake::with_language(FakeLanguageAdapter::single(script)).expect("fake");
        let error = fake
            .provider()
            .language_model("invalid-lifecycle")
            .expect("model")
            .generate(LanguageRequest::default())
            .await
            .expect_err("invalid lifecycle");
        assert_eq!(error.error.kind, ErrorKind::Protocol);
    }
}

#[derive(Debug)]
struct PendingAdapter;

impl LanguageAdapter for PendingAdapter {
    fn stream(
        &self,
        _call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        Box::pin(async { Ok(Box::pin(stream::pending()) as AdapterStream<_>) })
    }
}

#[tokio::test]
async fn explicit_abort_synthesizes_one_normal_aborted_terminal() {
    let provider = Provider::builder(
        DeploymentConfig::new("pending", "fake", "fake://pending")
            .with_default_language_protocol("pending"),
    )
    .language_adapter(PendingAdapter)
    .build()
    .expect("provider");
    let model = provider.language_model("model").expect("model");
    let mut generation = model.stream(LanguageRequest::default());
    assert!(matches!(
        generation.next().await,
        Some(Ok(GenerationEvent::Started { .. }))
    ));
    assert!(generation.abort());
    assert!(matches!(
        generation.next().await,
        Some(Ok(GenerationEvent::Finish {
            outcome: GenerationOutcome::Aborted,
            ..
        }))
    ));
    let output = generation.finish().await.expect("aborted output");
    assert_eq!(output.outcome, GenerationOutcome::Aborted);
}

#[test]
fn invocation_outside_tokio_runtime_keeps_started_first() {
    let fake = Fake::new().expect("fake");
    let model = fake.provider().language_model("model").expect("model");
    let mut generation = model.stream(LanguageRequest::default());
    let first = futures::executor::block_on(generation.next_event());
    assert!(matches!(first, Some(Ok(GenerationEvent::Started { .. }))));
    let second = futures::executor::block_on(generation.next_event());
    assert!(matches!(
        second,
        Some(Err(error)) if error.error.kind == ErrorKind::RuntimeUnavailable
    ));
}

#[tokio::test]
async fn fake_realtime_session_sends_and_closes_through_bounded_sender() {
    let fake = Fake::new().expect("fake");
    let model = fake.provider().realtime_model("realtime").expect("model");
    let mut session = model
        .connect(psychevo_ai::RealtimeConnectRequest {
            instructions: None,
            voice: None,
            headers: Default::default(),
            extensions: Default::default(),
        })
        .await
        .expect("session");
    let sender = session.sender();
    sender.send_text("hello").await.expect("text");
    assert!(matches!(
        session.next_event().await,
        Some(Ok(psychevo_ai::RealtimeEvent::InputTranscriptDelta { delta })) if delta == "hello"
    ));
    assert!(matches!(
        session.next_event().await,
        Some(Ok(psychevo_ai::RealtimeEvent::InputTranscriptDone { text })) if text == "hello"
    ));
    sender.close().await.expect("close");
    assert!(matches!(
        session.next_event().await,
        Some(Ok(psychevo_ai::RealtimeEvent::Closed {
            reason: psychevo_ai::RealtimeCloseReason::Requested,
        }))
    ));
    assert!(sender.send_text("late").await.is_err());
}

#[tokio::test]
async fn realtime_abort_has_one_normal_aborted_close() {
    let fake = Fake::new().expect("fake");
    let model = fake.provider().realtime_model("realtime").expect("model");
    let mut session = model
        .connect(RealtimeConnectRequest {
            instructions: None,
            voice: None,
            headers: Default::default(),
            extensions: Default::default(),
        })
        .await
        .expect("session");

    assert!(session.abort());
    assert!(matches!(
        session.next_event().await,
        Some(Ok(psychevo_ai::RealtimeEvent::Closed {
            reason: psychevo_ai::RealtimeCloseReason::Aborted,
        }))
    ));
    assert!(session.next_event().await.is_none());
}

#[derive(Debug)]
struct BackpressuredRealtimeSink {
    entered: Arc<tokio::sync::Semaphore>,
}

impl RealtimeCommandSink for BackpressuredRealtimeSink {
    fn send(&self, _command: RealtimeCommand) -> AdapterFuture<'_, ()> {
        self.entered.add_permits(1);
        Box::pin(std::future::pending())
    }

    fn close(&self) -> AdapterFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Debug)]
struct BackpressuredRealtimeAdapter {
    entered: Arc<tokio::sync::Semaphore>,
}

impl RealtimeAdapter for BackpressuredRealtimeAdapter {
    fn connect(
        &self,
        _call: AdapterCall<RealtimeConnectRequest>,
    ) -> AdapterFuture<'_, RealtimeAdapterTransport> {
        let commands = Arc::new(BackpressuredRealtimeSink {
            entered: self.entered.clone(),
        });
        Box::pin(async move {
            Ok(RealtimeAdapterTransport {
                commands,
                events: Box::pin(stream::pending()),
            })
        })
    }
}

#[tokio::test]
async fn realtime_queue_admission_deadline_is_timeout() {
    let entered = Arc::new(tokio::sync::Semaphore::new(0));
    let provider = Provider::builder(
        DeploymentConfig::new("realtime", "example", "example://realtime").with_timeout_policy(
            TimeoutPolicy {
                realtime_command_timeout_secs: 1,
                ..TimeoutPolicy::default()
            },
        ),
    )
    .realtime_adapter(BackpressuredRealtimeAdapter {
        entered: entered.clone(),
    })
    .build()
    .expect("provider");
    let session = provider
        .realtime_model("model")
        .expect("model")
        .connect(RealtimeConnectRequest {
            instructions: None,
            voice: None,
            headers: Default::default(),
            extensions: Default::default(),
        })
        .await
        .expect("session");
    let first = {
        let sender = session.sender();
        tokio::spawn(async move { sender.send_text("first").await })
    };
    entered
        .acquire()
        .await
        .expect("first command entered")
        .forget();

    let errors = join_all((0..33).map(|index| {
        let sender = session.sender();
        async move {
            sender
                .send_text(format!("queued-{index}"))
                .await
                .expect_err("backpressured command")
        }
    }))
    .await;

    assert!(
        errors.iter().all(|error| error.kind == ErrorKind::Timeout),
        "{errors:#?}"
    );
    assert_eq!(
        first
            .await
            .expect("first sender")
            .expect_err("first timeout")
            .kind,
        ErrorKind::Timeout
    );
}

#[test]
fn secret_debug_output_is_redacted() {
    let secret = psychevo_ai::SecretValue::new("do-not-print");
    assert_eq!(format!("{secret:?}"), "SecretValue([REDACTED])");
    let resolver: Arc<dyn psychevo_ai::CredentialResolver> =
        Arc::new(psychevo_ai::EmptyCredentialResolver);
    assert_eq!(Arc::strong_count(&resolver), 1);
}

#[derive(Debug)]
struct FixedImageAdapter {
    images: usize,
}

impl ImageAdapter for FixedImageAdapter {
    fn generate(&self, _call: AdapterCall<ImageRequest>) -> AdapterFuture<'_, ImageAdapterOutput> {
        let images = (0..self.images)
            .map(|_| GeneratedImage {
                media: Media::from_base64("image/png", "cG5n"),
                revised_prompt: None,
                provider_metadata: Default::default(),
            })
            .collect();
        Box::pin(async move {
            Ok(ImageAdapterOutput {
                images,
                usage: None,
                warnings: Vec::new(),
                provider_metadata: Default::default(),
            })
        })
    }
}

fn image_request(count: u32) -> ImageRequest {
    ImageRequest {
        prompt: "draw a test image".to_string(),
        count,
        aspect_ratio: None,
        input_images: Vec::new(),
        size: None,
        format: None,
        headers: Default::default(),
        extensions: Default::default(),
    }
}

fn image_provider(images: usize) -> Result<Provider, ProviderError> {
    Provider::builder(DeploymentConfig::new(
        "images",
        "example",
        "example://images",
    ))
    .image_adapter(FixedImageAdapter { images })
    .build()
}

#[tokio::test]
async fn image_result_with_fewer_items_is_successful_with_warning() {
    let model = image_provider(1)
        .expect("provider")
        .image_model("image-model")
        .expect("model");
    let output = model
        .generate(image_request(2))
        .await
        .expect("image output");
    assert_eq!(output.images.len(), 1);
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(output.warnings[0].code, "fewer_images_than_requested");
}

#[tokio::test]
async fn image_result_without_items_is_provider_failure() {
    let model = image_provider(0)
        .expect("provider")
        .image_model("image-model")
        .expect("model");
    let error = model
        .generate(image_request(1))
        .await
        .expect_err("zero images must fail");
    assert_eq!(error.kind, ErrorKind::Provider);
}

#[derive(Debug)]
struct InvalidMediaImageAdapter;

impl ImageAdapter for InvalidMediaImageAdapter {
    fn generate(&self, _call: AdapterCall<ImageRequest>) -> AdapterFuture<'_, ImageAdapterOutput> {
        Box::pin(async {
            Ok(ImageAdapterOutput {
                images: vec![GeneratedImage {
                    media: Media::from_base64("image/png", "not base64"),
                    revised_prompt: None,
                    provider_metadata: Default::default(),
                }],
                usage: None,
                warnings: Vec::new(),
                provider_metadata: Default::default(),
            })
        })
    }
}

#[tokio::test]
async fn invalid_provider_media_is_a_protocol_failure() {
    let provider = Provider::builder(DeploymentConfig::new(
        "invalid-media",
        "example",
        "example://invalid-media",
    ))
    .image_adapter(InvalidMediaImageAdapter)
    .build()
    .expect("provider");
    let error = provider
        .image_model("model")
        .expect("model")
        .generate(image_request(1))
        .await
        .expect_err("invalid media");
    assert_eq!(error.kind, ErrorKind::Protocol);
    assert_eq!(error.phase, ErrorPhase::ResponseBody);
}

#[test]
fn base64_backed_media_validates_on_first_access() {
    let media = Media::from_base64("image/png", "not base64");

    assert!(media.base64().is_err(), "Base64 access must validate input");
    assert!(
        serde_json::to_value(&media).is_err(),
        "serialization must not forward invalid media"
    );
}

#[derive(Debug)]
struct UnorderedTranscriptionAdapter;

impl TranscriptionAdapter for UnorderedTranscriptionAdapter {
    fn transcribe(
        &self,
        _call: AdapterCall<TranscriptionRequest>,
    ) -> AdapterFuture<'_, TranscriptionAdapterOutput> {
        Box::pin(async {
            Ok(TranscriptionAdapterOutput {
                text: "out of order".to_string(),
                segments: vec![
                    TranscriptionSegment {
                        start_seconds: 2.0,
                        end_seconds: 3.0,
                        text: "later".to_string(),
                    },
                    TranscriptionSegment {
                        start_seconds: 1.0,
                        end_seconds: 1.5,
                        text: "earlier".to_string(),
                    },
                ],
                language: None,
                duration_seconds: Some(3.0),
                warnings: Vec::new(),
                provider_metadata: Default::default(),
            })
        })
    }
}

#[tokio::test]
async fn unordered_transcription_segments_are_a_protocol_failure() {
    let provider = Provider::builder(DeploymentConfig::new(
        "transcription",
        "example",
        "example://transcription",
    ))
    .transcription_adapter(UnorderedTranscriptionAdapter)
    .build()
    .expect("provider");
    let error = provider
        .transcription_model("model")
        .expect("model")
        .transcribe(TranscriptionRequest {
            audio: psychevo_ai::MediaInput::Inline {
                media: Media::from_bytes("audio/wav", b"audio".to_vec()),
            },
            language: None,
            prompt: None,
            headers: Default::default(),
            extensions: Default::default(),
        })
        .await
        .expect_err("unordered segments");
    assert_eq!(error.kind, ErrorKind::Protocol);
}

#[derive(Debug)]
struct DelayedResolver;

impl CredentialResolver for DelayedResolver {
    fn resolve<'a>(&'a self, _request: CredentialRequest) -> AdapterFuture<'a, CredentialSnapshot> {
        Box::pin(async {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            Ok(CredentialSnapshot::default())
        })
    }
}

#[derive(Debug)]
struct DelayedImageAdapter;

impl ImageAdapter for DelayedImageAdapter {
    fn generate(&self, _call: AdapterCall<ImageRequest>) -> AdapterFuture<'_, ImageAdapterOutput> {
        Box::pin(async {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            Ok(ImageAdapterOutput {
                images: vec![GeneratedImage {
                    media: Media::from_base64("image/png", "cG5n"),
                    revised_prompt: None,
                    provider_metadata: Default::default(),
                }],
                usage: None,
                warnings: Vec::new(),
                provider_metadata: Default::default(),
            })
        })
    }
}

#[tokio::test]
async fn unary_total_deadline_spans_credentials_and_adapter_dispatch() {
    let provider = Provider::builder(
        DeploymentConfig::new("deadline", "example", "example://deadline").with_timeout_policy(
            TimeoutPolicy {
                progress_idle_timeout_secs: 0,
                total_deadline_secs: 1,
                ..TimeoutPolicy::default()
            },
        ),
    )
    .credential_resolver(Arc::new(DelayedResolver))
    .image_adapter(DelayedImageAdapter)
    .build()
    .expect("provider");
    let error = provider
        .image_model("model")
        .expect("model")
        .generate(image_request(1))
        .await
        .expect_err("one deadline must span both phases");
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert_eq!(error.phase, ErrorPhase::ResponseBody);
}

#[derive(Debug)]
struct CountingResolver {
    calls: Arc<AtomicUsize>,
}

impl CredentialResolver for CountingResolver {
    fn resolve<'a>(&'a self, request: CredentialRequest) -> AdapterFuture<'a, CredentialSnapshot> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            assert_eq!(request.deployment_id, "observed");
            Ok(CredentialSnapshot::new(BTreeMap::from([(
                CredentialSlot::new("api_key"),
                SecretValue::new("resolved-secret"),
            )])))
        })
    }
}

#[derive(Debug)]
struct AdapterObservation {
    headers: BTreeMap<String, String>,
    secret: String,
}

#[derive(Debug)]
struct ObservingAdapter {
    observations: Arc<Mutex<Vec<AdapterObservation>>>,
}

impl LanguageAdapter for ObservingAdapter {
    fn stream(
        &self,
        call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        self.observations
            .lock()
            .expect("observations")
            .push(AdapterObservation {
                headers: call.context.headers,
                secret: call
                    .context
                    .credentials
                    .require("api_key")
                    .expect("credential")
                    .expose_secret()
                    .to_string(),
            });
        Box::pin(async move {
            Ok(Box::pin(stream::iter([Ok(LanguageAdapterEvent::Finish {
                finish_reason: None,
            })])) as AdapterStream<_>)
        })
    }
}

#[tokio::test]
async fn credentials_resolve_once_and_request_headers_override_deployment_headers() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observations = Arc::new(Mutex::new(Vec::new()));
    let provider = Provider::builder(
        DeploymentConfig::new("observed", "example", "example://observed")
            .with_header("X-Trace", "deployment")
            .with_credentials(CredentialBindings::default().bind("api_key", "test/observed")),
    )
    .credential_resolver(Arc::new(CountingResolver {
        calls: calls.clone(),
    }))
    .language_adapter(ObservingAdapter {
        observations: observations.clone(),
    })
    .build()
    .expect("provider");
    let model = provider.language_model("model").expect("model");
    model
        .generate(LanguageRequest {
            headers: BTreeMap::from([
                ("x-trace".to_string(), "invocation".to_string()),
                ("x-feature".to_string(), "enabled".to_string()),
            ]),
            ..LanguageRequest::default()
        })
        .await
        .expect("generation");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let observations = observations.lock().expect("observations");
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].headers["x-trace"], "invocation");
    assert_eq!(observations[0].headers["x-feature"], "enabled");
    assert_eq!(observations[0].secret, "resolved-secret");
}

#[tokio::test]
async fn authentication_headers_are_rejected_before_credential_resolution() {
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = Provider::builder(
        DeploymentConfig::new("observed", "example", "example://observed")
            .with_credentials(CredentialBindings::default().bind("api_key", "test/observed")),
    )
    .credential_resolver(Arc::new(CountingResolver {
        calls: calls.clone(),
    }))
    .language_adapter(FakeLanguageAdapter::text("unused"))
    .build()
    .expect("provider");
    let error = provider
        .language_model("model")
        .expect("model")
        .generate(LanguageRequest {
            headers: BTreeMap::from([(
                "Authorization".to_string(),
                "Bearer must-not-pass".to_string(),
            )]),
            ..LanguageRequest::default()
        })
        .await
        .expect_err("authentication header");
    assert_eq!(error.error.kind, ErrorKind::Configuration);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[derive(Debug)]
struct NoopRealtimeSink;

impl RealtimeCommandSink for NoopRealtimeSink {
    fn send(&self, _command: RealtimeCommand) -> AdapterFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn close(&self) -> AdapterFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Debug)]
struct PrematureRealtimeEof;

impl RealtimeAdapter for PrematureRealtimeEof {
    fn connect(
        &self,
        _call: AdapterCall<RealtimeConnectRequest>,
    ) -> AdapterFuture<'_, RealtimeAdapterTransport> {
        Box::pin(async {
            Ok(RealtimeAdapterTransport {
                commands: Arc::new(NoopRealtimeSink),
                events: Box::pin(stream::empty::<Result<RealtimeAdapterEvent, ProviderError>>()),
            })
        })
    }
}

#[tokio::test]
async fn realtime_eof_before_closed_is_one_protocol_error() {
    let provider = Provider::builder(DeploymentConfig::new(
        "realtime",
        "example",
        "example://realtime",
    ))
    .realtime_adapter(PrematureRealtimeEof)
    .build()
    .expect("provider");
    let mut session = provider
        .realtime_model("model")
        .expect("model")
        .connect(RealtimeConnectRequest {
            instructions: None,
            voice: None,
            headers: Default::default(),
            extensions: Default::default(),
        })
        .await
        .expect("session");
    let error = session
        .next_event()
        .await
        .expect("terminal error")
        .expect_err("protocol error");
    assert_eq!(error.kind, ErrorKind::Protocol);
    assert!(session.next_event().await.is_none());
}

#[test]
fn registry_rejects_duplicates_and_ambiguous_targets() {
    let first = Provider::builder(DeploymentConfig::new("local", "example", "example://first"))
        .language_adapter(FakeLanguageAdapter::text("first"))
        .build()
        .expect("first");
    let duplicate = Provider::builder(DeploymentConfig::new(
        "local",
        "example",
        "example://second",
    ))
    .language_adapter(FakeLanguageAdapter::text("second"))
    .build()
    .expect("second");
    assert!(
        Registry::builder()
            .register(first.clone())
            .expect("first registration")
            .register(duplicate)
            .is_err()
    );
    let registry = Registry::builder()
        .register(first)
        .expect("registration")
        .build();
    assert!(registry.language_model("model-without-deployment").is_err());
    assert!(registry.language_model("local/").is_err());
    assert_eq!(
        registry
            .language_model("local/org/model")
            .expect("nested model")
            .descriptor()
            .model_id,
        "org/model"
    );
}

#[test]
fn provider_validation_rejects_invalid_identity_endpoint_headers_and_capability() {
    assert!(
        Provider::builder(DeploymentConfig::new(
            "Uppercase",
            "example",
            "example://local"
        ))
        .build()
        .is_err()
    );
    assert!(
        Provider::builder(DeploymentConfig::new(
            "userinfo",
            "example",
            "https://user:secret@example.test"
        ))
        .build()
        .is_err()
    );
    assert!(
        Provider::builder(
            DeploymentConfig::new("headers", "example", "example://local")
                .with_header("Authorization", "must-not-be-configured"),
        )
        .build()
        .is_err()
    );

    let provider = Provider::builder(DeploymentConfig::new(
        "language",
        "example",
        "example://local",
    ))
    .language_adapter(FakeLanguageAdapter::text("ok"))
    .build()
    .expect("provider");
    assert!(provider.image_model("model").is_err());
    assert!(provider.language_model(" model ").is_err());
}

#[tokio::test]
async fn empty_explicit_secret_is_rejected_during_credential_resolution() {
    let provider = Provider::builder(
        DeploymentConfig::new("secret", "example", "example://secret")
            .with_credentials(CredentialBindings::default().bind("api_key", "empty")),
    )
    .credential_resolver(Arc::new(psychevo_ai::StaticCredentialResolver::single(
        "empty",
        SecretValue::new(""),
    )))
    .language_adapter(FakeLanguageAdapter::text("unused"))
    .build()
    .expect("provider");

    let error = provider
        .language_model("model")
        .expect("model")
        .generate(LanguageRequest::default())
        .await
        .expect_err("empty secret");
    assert_eq!(error.error.kind, ErrorKind::Authentication);
    assert_eq!(error.error.phase, ErrorPhase::Credentials);
}

#[cfg(feature = "openai")]
#[tokio::test]
async fn openai_http_error_preserves_status_code_and_retry_after() {
    for protocol in ["chat", "responses"] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            let mut expected_length = None;
            loop {
                let read = stream.read(&mut buffer).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if expected_length.is_none()
                    && let Some(header_end) =
                        request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or_default();
                    expected_length = Some(header_end + 4 + content_length);
                }
                if expected_length.is_some_and(|length| request.len() >= length) {
                    break;
                }
            }
            let body = br#"{"error":{"message":"slow down","type":"rate_limit_error","code":"rate_limit_exceeded"}}"#;
            write!(
                stream,
                "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\nretry-after: 7\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            )
            .expect("write headers");
            stream.write_all(body).expect("write body");
            stream.flush().expect("flush");
        });
        let openai = OpenAi::builder(DeploymentConfig::new(
            "openai",
            "openai",
            format!("http://{address}/v1"),
        ))
        .http_client(
            reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("HTTP client"),
        )
        .with_api_key(SecretValue::new("test-key"))
        .build()
        .expect("OpenAI");
        let model = match protocol {
            "chat" => openai.chat("gpt-test"),
            "responses" => openai.responses("gpt-test"),
            _ => unreachable!(),
        }
        .expect("model");

        let error = model
            .generate(LanguageRequest::default())
            .await
            .expect_err("rate limit");

        assert_eq!(
            error.error.kind,
            ErrorKind::RateLimited,
            "{protocol}: {:?}",
            error.error
        );
        assert_eq!(error.error.status, Some(429), "{protocol}");
        assert_eq!(
            error.error.provider_code.as_deref(),
            Some("rate_limit_exceeded"),
            "{protocol}"
        );
        assert_eq!(error.error.retry_after_seconds, Some(7), "{protocol}");
        server.join().expect("server");
    }
}

#[cfg(feature = "openai")]
#[tokio::test]
async fn openai_image_error_preserves_retry_after() {
    let (endpoint, server) = single_http_response(
        "429 Too Many Requests",
        Some(9),
        r#"{"error":{"message":"slow down","code":"image_rate_limit"}}"#,
    );
    let openai = OpenAi::builder(DeploymentConfig::new("openai", "openai", endpoint))
        .http_client(
            reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("HTTP client"),
        )
        .with_api_key(SecretValue::new("test-key"))
        .build()
        .expect("OpenAI");

    let error = openai
        .image("gpt-image-test")
        .expect("image model")
        .generate(image_request(1))
        .await
        .expect_err("rate limit");

    assert_eq!(error.kind, ErrorKind::RateLimited);
    assert_eq!(error.status, Some(429));
    assert_eq!(error.provider_code.as_deref(), Some("image_rate_limit"));
    assert_eq!(error.retry_after_seconds, Some(9));
    server.join().expect("server");
}

#[cfg(feature = "anthropic")]
#[tokio::test]
async fn anthropic_error_preserves_retry_after() {
    let (endpoint, server) = single_http_response(
        "429 Too Many Requests",
        Some(11),
        r#"{"error":{"type":"rate_limit_error","message":"slow down"}}"#,
    );
    let anthropic = Anthropic::builder(DeploymentConfig::new("anthropic", "anthropic", endpoint))
        .runtime(
            ProviderRuntime::default().with_client(
                reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .expect("HTTP client"),
            ),
        )
        .with_api_key(SecretValue::new("test-key"))
        .build()
        .expect("Anthropic");

    let error = anthropic
        .messages("claude-test")
        .expect("model")
        .generate(LanguageRequest {
            messages: vec![Message::user("hello")],
            ..LanguageRequest::default()
        })
        .await
        .expect_err("rate limit");

    assert_eq!(error.error.kind, ErrorKind::RateLimited);
    assert_eq!(error.error.status, Some(429));
    assert_eq!(
        error.error.provider_code.as_deref(),
        Some("rate_limit_error")
    );
    assert_eq!(error.error.retry_after_seconds, Some(11));
    server.join().expect("server");
}

#[cfg(feature = "xiaomi")]
#[tokio::test]
async fn xiaomi_error_preserves_http_classification_and_retry_after() {
    let (endpoint, server) = single_http_response(
        "429 Too Many Requests",
        Some(13),
        r#"{"error":{"code":"voice_rate_limit","message":"slow down"}}"#,
    );
    let xiaomi = Xiaomi::builder(DeploymentConfig::new("xiaomi", "xiaomi", endpoint))
        .runtime(
            ProviderRuntime::default().with_client(
                reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .expect("HTTP client"),
            ),
        )
        .with_api_key(SecretValue::new("test-key"))
        .build()
        .expect("Xiaomi");

    let error = xiaomi
        .transcription("mimo-v2.5-asr")
        .expect("transcription model")
        .transcribe(TranscriptionRequest {
            audio: psychevo_ai::MediaInput::Inline {
                media: Media::from_bytes("audio/wav", b"wav".to_vec()),
            },
            language: None,
            prompt: None,
            headers: Default::default(),
            extensions: Default::default(),
        })
        .await
        .expect_err("rate limit");

    assert_eq!(error.kind, ErrorKind::RateLimited);
    assert_eq!(error.status, Some(429));
    assert_eq!(error.provider_code.as_deref(), Some("voice_rate_limit"));
    assert_eq!(error.retry_after_seconds, Some(13));
    server.join().expect("server");
}
