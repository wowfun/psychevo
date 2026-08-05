use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Configuration;
pub use crate::config::{VoiceAudioFormat, VoiceRealtimeTransport};
use crate::{Error, Result, config};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceAudioInput {
    pub data: String,
    pub format: VoiceAudioFormat,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceAudioOutput {
    pub data: String,
    pub format: VoiceAudioFormat,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceTranscriptionRequest {
    pub audio: VoiceAudioInput,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceTranscription {
    pub transcript: String,
    pub provider: String,
    pub model: String,
    pub language: Option<String>,
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSpeechRequest {
    pub text: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub voice: Option<String>,
    pub format: Option<VoiceAudioFormat>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSpeech {
    pub audio: VoiceAudioOutput,
    pub provider: String,
    pub model: String,
    pub voice: String,
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceRealtimeRequest {
    pub thread_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub transport: Option<VoiceRealtimeTransport>,
    pub voice: Option<String>,
    pub sdp_offer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceRealtimeCloseReason {
    Requested,
    Remote,
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum VoiceRealtimeEvent {
    InputTranscriptDelta { delta: String },
    InputTranscriptDone { text: String },
    OutputTextDelta { delta: String },
    OutputTextDone { text: String },
    OutputAudioDelta { audio: VoiceAudioOutput },
    OutputAudioDone,
    ResponseDone,
    Warning { message: String },
    Metadata { metadata: BTreeMap<String, Value> },
    Closed { reason: VoiceRealtimeCloseReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceRealtimeVoice {
    pub id: String,
    pub label: String,
}

pub struct VoiceRealtimeConnection {
    pub provider: String,
    pub control: VoiceRealtimeControl,
    pub events: VoiceRealtimeEvents,
}

impl std::fmt::Debug for VoiceRealtimeConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VoiceRealtimeConnection")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct VoiceRealtimeControl {
    provider: String,
    sender: psychevo_ai::RealtimeSender,
}

impl std::fmt::Debug for VoiceRealtimeControl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VoiceRealtimeControl")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

pub struct VoiceRealtimeEvents {
    session: psychevo_ai::RealtimeSession,
}

impl std::fmt::Debug for VoiceRealtimeEvents {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("VoiceRealtimeEvents { .. }")
    }
}

impl Configuration {
    pub async fn transcribe_voice(
        &self,
        request: VoiceTranscriptionRequest,
    ) -> Result<VoiceTranscription> {
        let resolved = config::resolve_voice_asr_config(
            &self.options,
            request.provider.as_deref(),
            request.model.as_deref(),
            request.language.as_deref(),
        )?;
        let mime_type =
            validated_asr_mime_type(request.audio.format, request.audio.mime_type.as_deref())?;
        let provider = self.voice_provider(
            &resolved.provider,
            &resolved.base_url,
            resolved.api_key.as_deref(),
            &resolved.api_key_env,
        )?;
        let result = provider
            .transcription_model(resolved.model)
            .map_err(voice_provider_error)?
            .transcribe(psychevo_ai::TranscriptionRequest {
                audio: psychevo_ai::MediaInput::Inline {
                    media: psychevo_ai::Media::from_base64(mime_type, request.audio.data),
                },
                language: resolved.language,
                prompt: None,
                headers: BTreeMap::new(),
                extensions: BTreeMap::new(),
            })
            .await
            .map_err(voice_provider_error)?;
        Ok(VoiceTranscription {
            transcript: result.text,
            provider: result.model.provider_family,
            model: result.model.model_id,
            language: result.language,
            metadata: result.provider_metadata,
        })
    }

    pub async fn synthesize_voice(&self, request: VoiceSpeechRequest) -> Result<VoiceSpeech> {
        let resolved = config::resolve_voice_tts_config(
            &self.options,
            request.provider.as_deref(),
            request.model.as_deref(),
            request.voice.as_deref(),
            request.format,
        )?;
        let provider = self.voice_provider(
            &resolved.provider,
            &resolved.base_url,
            resolved.api_key.as_deref(),
            &resolved.api_key_env,
        )?;
        let result = provider
            .speech_model(resolved.model)
            .map_err(voice_provider_error)?
            .synthesize(psychevo_ai::SpeechRequest {
                text: request.text,
                voice: Some(resolved.voice.clone()),
                format: Some(resolved.format.as_str().to_string()),
                speed: None,
                headers: BTreeMap::new(),
                extensions: BTreeMap::new(),
            })
            .await
            .map_err(voice_provider_error)?;
        Ok(VoiceSpeech {
            audio: VoiceAudioOutput {
                data: result
                    .audio
                    .base64()
                    .map_err(|error| Error::Message(error.to_string()))?
                    .to_string(),
                format: resolved.format,
                mime_type: result.audio.mime_type().to_string(),
            },
            provider: result.model.provider_family,
            model: result.model.model_id,
            voice: resolved.voice,
            metadata: result.provider_metadata,
        })
    }

    pub async fn connect_realtime_voice(
        &self,
        request: VoiceRealtimeRequest,
    ) -> Result<VoiceRealtimeConnection> {
        let resolved = config::resolve_voice_realtime_config(
            &self.options,
            request.provider.as_deref(),
            request.model.as_deref(),
            request.transport,
            request.voice.as_deref(),
        )?
        .ok_or_else(|| Error::Config("voice.realtime is not configured".to_string()))?;
        let provider = if let Some(provider) = self.injected_voice_provider(&resolved.provider) {
            provider
        } else if resolved.provider == "fake" {
            fake_voice_provider()?
        } else {
            return Err(Error::Config(format!(
                "provider-native realtime is not available for {} in this build",
                resolved.provider
            )));
        };
        let session = provider
            .realtime_model(resolved.model)
            .map_err(voice_provider_error)?
            .connect(psychevo_ai::RealtimeConnectRequest {
                instructions: None,
                voice: resolved.voice,
                headers: BTreeMap::new(),
                extensions: BTreeMap::from([(
                    "psychevo".to_string(),
                    json!({
                        "thread_id": request.thread_id,
                        "transport": match resolved.transport {
                            VoiceRealtimeTransport::Webrtc => "webrtc",
                            VoiceRealtimeTransport::Websocket => "websocket",
                        },
                        "sdp_offer": request.sdp_offer,
                    }),
                )]),
            })
            .await
            .map_err(voice_provider_error)?;
        let control = VoiceRealtimeControl {
            provider: resolved.provider.clone(),
            sender: session.sender(),
        };
        Ok(VoiceRealtimeConnection {
            provider: resolved.provider,
            control,
            events: VoiceRealtimeEvents { session },
        })
    }

    fn injected_voice_provider(&self, selected: &str) -> Option<psychevo_ai::Provider> {
        self.provider.as_ref().and_then(|provider| {
            let deployment = provider.deployment_config();
            (deployment.deployment_id == selected || deployment.provider_family == selected)
                .then(|| provider.clone())
        })
    }

    fn voice_provider(
        &self,
        provider: &str,
        base_url: &str,
        api_key: Option<&str>,
        api_key_env: &Option<String>,
    ) -> Result<psychevo_ai::Provider> {
        if let Some(provider) = self.injected_voice_provider(provider) {
            return Ok(provider);
        }
        if provider == "fake" {
            return fake_voice_provider();
        }
        if !matches!(provider, "xiaomi" | "xiaomi-token-plan") {
            return Err(Error::Config(format!(
                "voice provider is not supported yet: {provider}"
            )));
        }
        let api_key = api_key.ok_or_else(|| {
            Error::Config(format!(
                "missing {}",
                api_key_env
                    .as_deref()
                    .unwrap_or("voice provider credentials")
            ))
        })?;
        psychevo_ai::Xiaomi::builder(
            psychevo_ai::DeploymentConfig::new(provider, provider, base_url)
                .with_default_language_protocol("xiaomi_voice"),
        )
        .with_api_key(psychevo_ai::SecretValue::new(api_key))
        .build()
        .and_then(|facade| facade.provider())
        .map_err(voice_provider_error)
    }
}

impl VoiceRealtimeControl {
    pub async fn append_audio(&self, audio: VoiceAudioInput) -> Result<()> {
        let mime_type = audio
            .mime_type
            .unwrap_or_else(|| audio.format.mime_type().to_string());
        self.sender
            .send_audio(psychevo_ai::Media::from_base64(mime_type, audio.data))
            .await
            .map_err(voice_provider_error)
    }

    pub async fn append_text(&self, text: impl Into<String>) -> Result<()> {
        self.sender
            .send_text(text)
            .await
            .map_err(voice_provider_error)
    }

    pub async fn append_speech(&self, text: impl Into<String>) -> Result<()> {
        self.append_text(text).await?;
        self.sender.commit().await.map_err(voice_provider_error)
    }

    pub async fn close(&self) -> Result<()> {
        self.sender.close().await.map_err(voice_provider_error)
    }

    pub fn voices(&self) -> Vec<VoiceRealtimeVoice> {
        if self.provider == "fake" {
            vec![VoiceRealtimeVoice {
                id: "fake".to_string(),
                label: "Fake voice".to_string(),
            }]
        } else {
            Vec::new()
        }
    }
}

impl VoiceRealtimeEvents {
    pub async fn next_event(&mut self) -> Option<Result<VoiceRealtimeEvent>> {
        let event = self.session.next_event().await?;
        Some(
            event
                .map_err(voice_provider_error)
                .and_then(map_realtime_event),
        )
    }
}

fn fake_voice_provider() -> Result<psychevo_ai::Provider> {
    psychevo_ai::Provider::builder(
        psychevo_ai::DeploymentConfig::new("fake", "fake", "fake://local")
            .with_default_language_protocol("fake"),
    )
    .transcription_adapter(psychevo_ai::FakeTranscriptionAdapter::default())
    .speech_adapter(psychevo_ai::FakeSpeechAdapter::new(
        psychevo_ai::Media::from_base64("audio/wav", "UklGRg=="),
    ))
    .realtime_adapter(psychevo_ai::FakeRealtimeAdapter)
    .build()
    .map_err(voice_provider_error)
}

fn validated_asr_mime_type(
    format: VoiceAudioFormat,
    declared_mime_type: Option<&str>,
) -> Result<String> {
    if !format.supports_asr_input() {
        return Err(Error::Message(format!(
            "unsupported ASR audio format `{}`",
            format.as_str()
        )));
    }
    if let Some(declared) = declared_mime_type {
        let declared = declared.trim().to_ascii_lowercase();
        let matches = match format {
            VoiceAudioFormat::Wav => {
                matches!(
                    declared.as_str(),
                    "audio/wav" | "audio/wave" | "audio/x-wav"
                )
            }
            VoiceAudioFormat::Mp3 => matches!(declared.as_str(), "audio/mpeg" | "audio/mp3"),
            VoiceAudioFormat::Pcm16 => false,
        };
        if !matches {
            return Err(Error::Message(format!(
                "ASR audio format `{}` conflicts with MIME type `{declared}`",
                format.as_str()
            )));
        }
    }
    Ok(format.mime_type().to_string())
}

fn map_realtime_event(event: psychevo_ai::RealtimeEvent) -> Result<VoiceRealtimeEvent> {
    Ok(match event {
        psychevo_ai::RealtimeEvent::InputTranscriptDelta { delta } => {
            VoiceRealtimeEvent::InputTranscriptDelta { delta }
        }
        psychevo_ai::RealtimeEvent::InputTranscriptDone { text } => {
            VoiceRealtimeEvent::InputTranscriptDone { text }
        }
        psychevo_ai::RealtimeEvent::OutputTextDelta { delta } => {
            VoiceRealtimeEvent::OutputTextDelta { delta }
        }
        psychevo_ai::RealtimeEvent::OutputTextDone { text } => {
            VoiceRealtimeEvent::OutputTextDone { text }
        }
        psychevo_ai::RealtimeEvent::OutputAudioDelta { audio } => {
            VoiceRealtimeEvent::OutputAudioDelta {
                audio: VoiceAudioOutput {
                    data: audio
                        .base64()
                        .map_err(|error| Error::Message(error.to_string()))?
                        .to_string(),
                    format: voice_format_from_mime(audio.mime_type()),
                    mime_type: audio.mime_type().to_string(),
                },
            }
        }
        psychevo_ai::RealtimeEvent::OutputAudioDone => VoiceRealtimeEvent::OutputAudioDone,
        psychevo_ai::RealtimeEvent::ResponseDone => VoiceRealtimeEvent::ResponseDone,
        psychevo_ai::RealtimeEvent::Warning { warning } => VoiceRealtimeEvent::Warning {
            message: warning.message,
        },
        psychevo_ai::RealtimeEvent::Metadata { metadata } => {
            VoiceRealtimeEvent::Metadata { metadata }
        }
        psychevo_ai::RealtimeEvent::Closed { reason } => VoiceRealtimeEvent::Closed {
            reason: match reason {
                psychevo_ai::RealtimeCloseReason::Requested => VoiceRealtimeCloseReason::Requested,
                psychevo_ai::RealtimeCloseReason::Remote => VoiceRealtimeCloseReason::Remote,
                psychevo_ai::RealtimeCloseReason::Aborted => VoiceRealtimeCloseReason::Aborted,
            },
        },
    })
}

fn voice_format_from_mime(mime_type: &str) -> VoiceAudioFormat {
    match mime_type {
        "audio/mpeg" | "audio/mp3" => VoiceAudioFormat::Mp3,
        "audio/pcm" | "audio/l16" => VoiceAudioFormat::Pcm16,
        _ => VoiceAudioFormat::Wav,
    }
}

fn voice_provider_error(error: impl std::fmt::Display) -> Error {
    Error::Message(format!("voice provider failed: {error}"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use futures::stream;

    use super::*;
    use crate::application::{Application, ConfigurationQuery};

    #[derive(Debug)]
    struct AcceptanceRealtimeAdapter {
        accepted: Arc<AtomicBool>,
    }

    impl psychevo_ai::RealtimeAdapter for AcceptanceRealtimeAdapter {
        fn connect(
            &self,
            _call: psychevo_ai::AdapterCall<psychevo_ai::RealtimeConnectRequest>,
        ) -> psychevo_ai::AdapterFuture<'_, psychevo_ai::RealtimeAdapterTransport> {
            let accepted = Arc::clone(&self.accepted);
            Box::pin(async move {
                Ok(psychevo_ai::RealtimeAdapterTransport {
                    commands: Arc::new(AcceptanceRealtimeSink { accepted }),
                    events: Box::pin(stream::pending()),
                })
            })
        }
    }

    struct AcceptanceRealtimeSink {
        accepted: Arc<AtomicBool>,
    }

    impl psychevo_ai::RealtimeCommandSink for AcceptanceRealtimeSink {
        fn send(
            &self,
            _command: psychevo_ai::RealtimeCommand,
        ) -> psychevo_ai::AdapterFuture<'_, ()> {
            self.accepted.store(true, Ordering::Release);
            Box::pin(async { Ok(()) })
        }

        fn close(&self) -> psychevo_ai::AdapterFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn realtime_control_acknowledges_only_after_adapter_acceptance() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::write(
            home.join("config.toml"),
            r#"
[provider.acceptance]
name = "Acceptance"
api = "https://example.test"
no_auth = true

[voice.realtime]
provider = "acceptance"
model = "test"
transport = "websocket"
"#,
        )
        .expect("config");
        let accepted = Arc::new(AtomicBool::new(false));
        let provider = psychevo_ai::Provider::builder(psychevo_ai::DeploymentConfig::new(
            "acceptance",
            "test",
            "test://realtime",
        ))
        .realtime_adapter(AcceptanceRealtimeAdapter {
            accepted: Arc::clone(&accepted),
        })
        .build()
        .expect("provider");
        let application = Application::builder()
            .home(&home)
            .provider(provider)
            .build()
            .await
            .expect("application");
        let configuration = application
            .client()
            .configuration(ConfigurationQuery::new(cwd))
            .expect("configuration");
        let connection = configuration
            .connect_realtime_voice(VoiceRealtimeRequest {
                thread_id: "thread-1".to_string(),
                provider: None,
                model: None,
                transport: None,
                voice: None,
                sdp_offer: None,
            })
            .await
            .expect("connect");

        connection
            .control
            .append_text("queued")
            .await
            .expect("append");

        assert!(
            accepted.load(Ordering::Acquire),
            "successful append must follow Adapter command acceptance"
        );
        drop(connection);
        application
            .shutdown()
            .await
            .expect("shutdown")
            .require_clean()
            .expect("clean shutdown");
    }
}
