use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use futures::stream;
use tokio::sync::mpsc;

use crate::{
    AdapterCall, AdapterFuture, AdapterResult, AdapterStream, DeploymentConfig, GeneratedImage,
    ImageAdapter, ImageAdapterOutput, ImageRequest, LanguageAdapter, LanguageAdapterEvent, Media,
    Provider, ProviderError, RealtimeAdapter, RealtimeAdapterEvent, RealtimeAdapterTransport,
    RealtimeCommand, RealtimeCommandSink, RealtimeConnectRequest, SpeechAdapter,
    SpeechAdapterOutput, SpeechRequest, TranscriptionAdapter, TranscriptionAdapterOutput,
};

pub const DEFAULT_FAKE_IMAGE_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mOsvmfPfwAH5QMm7n0ViwAAAABJRU5ErkJggg==";

#[derive(Clone, Default)]
pub struct FakeLanguageAdapter {
    scripts: Arc<Mutex<VecDeque<Vec<AdapterResult<LanguageAdapterEvent>>>>>,
}

impl std::fmt::Debug for FakeLanguageAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FakeLanguageAdapter")
            .field(
                "remaining_scripts",
                &self.scripts.lock().expect("fake scripts").len(),
            )
            .finish()
    }
}

impl FakeLanguageAdapter {
    pub fn new(
        scripts: impl IntoIterator<Item = Vec<AdapterResult<LanguageAdapterEvent>>>,
    ) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(scripts.into_iter().collect())),
        }
    }

    pub fn single(events: Vec<LanguageAdapterEvent>) -> Self {
        Self::new([events.into_iter().map(Ok).collect()])
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::single(vec![
            LanguageAdapterEvent::TextStart { content_index: 0 },
            LanguageAdapterEvent::TextDelta {
                content_index: 0,
                delta: text.into(),
            },
            LanguageAdapterEvent::TextEnd { content_index: 0 },
            LanguageAdapterEvent::Finish {
                finish_reason: None,
            },
        ])
    }
}

impl LanguageAdapter for FakeLanguageAdapter {
    fn stream(
        &self,
        _call: AdapterCall<crate::LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        let script = self.scripts.lock().expect("fake scripts").pop_front();
        Box::pin(async move {
            let script = script
                .ok_or_else(|| ProviderError::protocol("fake language Adapter script exhausted"))?;
            Ok(Box::pin(stream::iter(script)) as AdapterStream<_>)
        })
    }
}

#[derive(Debug, Clone)]
pub struct FakeImageAdapter {
    media: Media,
}

impl Default for FakeImageAdapter {
    fn default() -> Self {
        Self {
            media: Media::from_base64("image/png", crate::DEFAULT_FAKE_IMAGE_BASE64),
        }
    }
}

impl FakeImageAdapter {
    pub fn new(media: Media) -> Self {
        Self { media }
    }
}

impl ImageAdapter for FakeImageAdapter {
    fn generate(&self, call: AdapterCall<ImageRequest>) -> AdapterFuture<'_, ImageAdapterOutput> {
        let media = self.media.clone();
        Box::pin(async move {
            let provider_metadata = BTreeMap::from([
                (
                    "provider".to_string(),
                    serde_json::Value::String("fake".to_string()),
                ),
                (
                    "input_images".to_string(),
                    serde_json::json!(call.request.input_images.len()),
                ),
                (
                    "aspect_ratio".to_string(),
                    serde_json::json!(call.request.aspect_ratio),
                ),
                ("size".to_string(), serde_json::json!(call.request.size)),
                ("format".to_string(), serde_json::json!(call.request.format)),
            ]);
            Ok(ImageAdapterOutput {
                images: (0..call.request.count)
                    .map(|_| GeneratedImage {
                        media: media.clone(),
                        revised_prompt: Some(format!("fake: {}", call.request.prompt)),
                        provider_metadata: BTreeMap::new(),
                    })
                    .collect(),
                usage: None,
                warnings: Vec::new(),
                provider_metadata,
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct FakeTranscriptionAdapter {
    transcript: String,
}

impl Default for FakeTranscriptionAdapter {
    fn default() -> Self {
        Self::new("fake transcript")
    }
}

impl FakeTranscriptionAdapter {
    pub fn new(transcript: impl Into<String>) -> Self {
        Self {
            transcript: transcript.into(),
        }
    }
}

impl TranscriptionAdapter for FakeTranscriptionAdapter {
    fn transcribe(
        &self,
        call: AdapterCall<crate::TranscriptionRequest>,
    ) -> AdapterFuture<'_, TranscriptionAdapterOutput> {
        let transcript = self.transcript.clone();
        Box::pin(async move {
            Ok(TranscriptionAdapterOutput {
                text: transcript,
                segments: Vec::new(),
                language: call.request.language,
                duration_seconds: None,
                warnings: Vec::new(),
                provider_metadata: BTreeMap::from([(
                    "provider".to_string(),
                    serde_json::Value::String("fake".to_string()),
                )]),
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct FakeSpeechAdapter {
    audio: Media,
}

impl Default for FakeSpeechAdapter {
    fn default() -> Self {
        Self {
            audio: Media::from_bytes("audio/wav", b"fake audio".to_vec()),
        }
    }
}

impl FakeSpeechAdapter {
    pub fn new(audio: Media) -> Self {
        Self { audio }
    }
}

impl SpeechAdapter for FakeSpeechAdapter {
    fn synthesize(
        &self,
        _call: AdapterCall<SpeechRequest>,
    ) -> AdapterFuture<'_, SpeechAdapterOutput> {
        let audio = self.audio.clone();
        Box::pin(async move {
            Ok(SpeechAdapterOutput {
                audio,
                warnings: Vec::new(),
                provider_metadata: BTreeMap::from([(
                    "provider".to_string(),
                    serde_json::Value::String("fake".to_string()),
                )]),
            })
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FakeRealtimeAdapter;

impl RealtimeAdapter for FakeRealtimeAdapter {
    fn connect(
        &self,
        _call: AdapterCall<RealtimeConnectRequest>,
    ) -> AdapterFuture<'_, RealtimeAdapterTransport> {
        Box::pin(async move {
            let (events, receiver) = mpsc::unbounded_channel();
            let sink = Arc::new(FakeRealtimeCommandSink { events });
            let stream = stream::unfold(receiver, |mut receiver| async move {
                receiver.recv().await.map(|event| (Ok(event), receiver))
            });
            Ok(RealtimeAdapterTransport {
                commands: sink,
                events: Box::pin(stream),
            })
        })
    }
}

struct FakeRealtimeCommandSink {
    events: mpsc::UnboundedSender<RealtimeAdapterEvent>,
}

impl RealtimeCommandSink for FakeRealtimeCommandSink {
    fn send(&self, command: RealtimeCommand) -> AdapterFuture<'_, ()> {
        Box::pin(async move {
            let events = match command {
                RealtimeCommand::InputText { text } => vec![
                    RealtimeAdapterEvent::InputTranscriptDelta {
                        delta: text.clone(),
                    },
                    RealtimeAdapterEvent::InputTranscriptDone { text },
                ],
                RealtimeCommand::InputAudio { .. } => {
                    vec![RealtimeAdapterEvent::InputTranscriptDone {
                        text: "fake realtime transcript".to_string(),
                    }]
                }
                RealtimeCommand::Commit => vec![RealtimeAdapterEvent::ResponseDone],
            };
            for event in events {
                self.events.send(event).map_err(|_| {
                    ProviderError::protocol("fake realtime event receiver was dropped")
                })?;
            }
            Ok(())
        })
    }

    fn close(&self) -> AdapterFuture<'_, ()> {
        Box::pin(async move {
            self.events
                .send(RealtimeAdapterEvent::Closed { remote: false })
                .map_err(|_| ProviderError::protocol("fake realtime event receiver was dropped"))
        })
    }
}

#[derive(Debug, Clone)]
pub struct Fake {
    provider: Provider,
}

impl Fake {
    pub fn new() -> Result<Self, ProviderError> {
        Self::with_language(FakeLanguageAdapter::text("fake response"))
    }

    pub fn with_language(language: FakeLanguageAdapter) -> Result<Self, ProviderError> {
        let provider = Provider::builder(
            DeploymentConfig::new("fake", "fake", "fake://local")
                .with_default_language_protocol("fake"),
        )
        .language_adapter(language)
        .image_adapter(FakeImageAdapter::default())
        .transcription_adapter(FakeTranscriptionAdapter::default())
        .speech_adapter(FakeSpeechAdapter::default())
        .realtime_adapter(FakeRealtimeAdapter)
        .build()?;
        Ok(Self { provider })
    }

    pub fn provider(&self) -> Provider {
        self.provider.clone()
    }
}
