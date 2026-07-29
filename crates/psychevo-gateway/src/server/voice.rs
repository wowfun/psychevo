use super::*;
use psychevo::__ai::{
    DeploymentConfig, Fake, FakeSpeechAdapter, FakeTranscriptionAdapter, Media, MediaInput,
    Provider, RealtimeCloseReason, RealtimeConnectRequest, RealtimeEvent, RealtimeSender,
    SecretValue, SpeechRequest, TranscriptionRequest, Xiaomi,
};

#[derive(Debug, Clone)]
pub(super) struct RealtimeSessionState {
    pub(super) provider: String,
    pub(super) sender: RealtimeSender,
}

pub(super) async fn voice_asr_transcribe_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::VoiceAsrTranscribeParams,
) -> psychevo::Result<Value> {
    let scope = resolve_optional_scope(state, auth, params.scope.clone())?;
    let options = state.run_options(scope.cwd, None);
    let resolved = resolve_voice_asr_config(
        &options,
        params.provider.as_deref(),
        params.model.as_deref(),
        params.language.as_deref(),
    )?;
    let audio_format = ai_audio_format(params.audio.format);
    let mime_type = validated_asr_mime_type(audio_format, params.audio.mime_type.as_deref())?;
    let request = TranscriptionRequest {
        audio: MediaInput::Inline {
            media: Media::from_base64(mime_type, params.audio.data),
        },
        language: resolved.language.clone(),
        prompt: None,
        headers: BTreeMap::new(),
        extensions: BTreeMap::new(),
    };
    let provider = voice_provider(
        &resolved.provider,
        &resolved.base_url,
        resolved.api_key.as_deref(),
        &resolved.api_key_env,
    )?;
    let model = provider
        .transcription_model(resolved.model.clone())
        .map_err(voice_runtime_error)?;
    let result = model
        .transcribe(request)
        .await
        .map_err(voice_runtime_error)?;
    Ok(serde_json::to_value(wire::VoiceAsrTranscribeResult {
        transcript: result.text,
        provider: result.model.provider_family,
        model: result.model.model_id,
        language: result.language,
        metadata: Some(Value::Object(
            result.provider_metadata.into_iter().collect(),
        )),
    })?)
}

pub(super) async fn voice_tts_synthesize_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::VoiceTtsSynthesizeParams,
) -> psychevo::Result<Value> {
    let scope = resolve_optional_scope(state, auth, params.scope.clone())?;
    let options = state.run_options(scope.cwd, None);
    let resolved = resolve_voice_tts_config(
        &options,
        params.provider.as_deref(),
        params.model.as_deref(),
        params.voice.as_deref(),
        params.format.map(ai_audio_format),
    )?;
    let request = SpeechRequest {
        text: params.text,
        voice: Some(resolved.voice.clone()),
        format: Some(resolved.format.as_str().to_string()),
        speed: None,
        headers: BTreeMap::new(),
        extensions: BTreeMap::new(),
    };
    let provider = voice_provider(
        &resolved.provider,
        &resolved.base_url,
        resolved.api_key.as_deref(),
        &resolved.api_key_env,
    )?;
    let model = provider
        .speech_model(resolved.model.clone())
        .map_err(voice_runtime_error)?;
    let result = model
        .synthesize(request)
        .await
        .map_err(voice_runtime_error)?;
    Ok(serde_json::to_value(wire::VoiceTtsSynthesizeResult {
        audio: wire::VoiceAudioOutput {
            data: result
                .audio
                .base64()
                .map_err(|error| Error::Message(error.to_string()))?
                .to_string(),
            format: wire_audio_format(resolved.format),
            mime_type: result.audio.mime_type().to_string(),
        },
        provider: result.model.provider_family,
        model: result.model.model_id,
        voice: resolved.voice,
        metadata: Some(Value::Object(
            result.provider_metadata.into_iter().collect(),
        )),
    })?)
}

pub(super) async fn voice_policy_read_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::VoicePolicyReadParams,
) -> psychevo::Result<Value> {
    let target = voice_policy_target(
        state,
        auth,
        params.scope,
        params.source_key,
        params.thread_id,
    )
    .await?;
    let mode = state
        .inner
        .voice_policies
        .lock()
        .expect("voice policies poisoned")
        .get(&target)
        .copied()
        .unwrap_or(wire::VoicePolicyMode::Off);
    Ok(serde_json::to_value(wire::VoicePolicyResult {
        mode,
        target,
    })?)
}

pub(super) async fn voice_policy_update_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::VoicePolicyUpdateParams,
) -> psychevo::Result<Value> {
    let target = voice_policy_target(
        state,
        auth,
        params.scope,
        params.source_key,
        params.thread_id,
    )
    .await?;
    let mut policies = state
        .inner
        .voice_policies
        .lock()
        .expect("voice policies poisoned");
    if params.mode == wire::VoicePolicyMode::Off {
        policies.remove(&target);
    } else {
        policies.insert(target.clone(), params.mode);
    }
    Ok(serde_json::to_value(wire::VoicePolicyResult {
        mode: params.mode,
        target,
    })?)
}

pub(super) async fn start_realtime(
    state: &WebState,
    auth: &AuthContext,
    out_tx: ConnectionSender,
    params: wire::ThreadRealtimeStartParams,
) -> psychevo::Result<wire::ThreadRealtimeStartResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    let scope = resolve_optional_scope(state, auth, params.scope.clone())?;
    let mut options = state.run_options(scope.cwd, Some(params.thread_id.clone()));
    options.config_path = Some(state.inner.home.join("config.toml"));
    let resolved = resolve_voice_realtime_config(
        &options,
        params.provider.as_deref(),
        params.model.as_deref(),
        params.transport.map(ai_realtime_transport),
        params.voice.as_deref(),
    )?
    .ok_or_else(|| Error::Config("voice.realtime is not configured".to_string()))?;
    if resolved.provider != "fake" {
        return Err(Error::Config(format!(
            "provider-native realtime is not available for {} in this build",
            resolved.provider
        )));
    }
    let model = Fake::new()
        .map_err(voice_runtime_error)?
        .provider()
        .realtime_model(resolved.model.clone())
        .map_err(voice_runtime_error)?;
    let mut stream = model
        .connect(RealtimeConnectRequest {
            instructions: None,
            voice: resolved.voice.clone(),
            headers: BTreeMap::new(),
            extensions: BTreeMap::from([(
                "psychevo".to_string(),
                json!({
                    "thread_id": params.thread_id,
                    "transport": match resolved.transport {
                        psychevo::__product::configuration::VoiceRealtimeTransport::Webrtc => "webrtc",
                        psychevo::__product::configuration::VoiceRealtimeTransport::Websocket => "websocket",
                    },
                    "sdp_offer": params.sdp_offer,
                }),
            )]),
        })
        .await
        .map_err(voice_runtime_error)?;
    let session_id = format!("fake-realtime-{}", params.thread_id);
    let sender = stream.sender();
    state
        .inner
        .realtime_sessions
        .lock()
        .expect("realtime sessions poisoned")
        .insert(
            session_id.clone(),
            RealtimeSessionState {
                provider: resolved.provider,
                sender,
            },
        );
    let _ = out_tx.send(rpc_notification(
        "thread/realtime/started",
        json!(wire::ThreadRealtimeStartedNotification {
            session_id: session_id.clone(),
            thread_id: params.thread_id.clone(),
        }),
    ));
    let thread_id = params.thread_id.clone();
    let state_for_close = state.clone();
    let event_session_id = session_id.clone();
    let cleanup_session_id = session_id.clone();
    let supervisor = state.inner.gateway.clone();
    supervisor.spawn_background(format!("realtime-voice:{session_id}"), async move {
        while let Some(event) = stream.next_event().await {
            let Ok(event) = event else {
                continue;
            };
            let should_send = match &event {
                RealtimeEvent::Closed { .. } => state_for_close
                    .inner
                    .realtime_sessions
                    .lock()
                    .expect("realtime sessions poisoned")
                    .contains_key(&event_session_id),
                _ => true,
            };
            if !should_send {
                continue;
            }
            if let Some(notification) =
                realtime_event_notification(&event_session_id, &thread_id, event)
            {
                let _ = out_tx.send(notification);
            }
        }
        state_for_close
            .inner
            .realtime_sessions
            .lock()
            .expect("realtime sessions poisoned")
            .remove(&cleanup_session_id);
    });
    Ok(wire::ThreadRealtimeStartResult {
        accepted: true,
        session_id,
        thread_id: params.thread_id,
    })
}

pub(super) async fn append_realtime_audio(
    state: &WebState,
    params: wire::ThreadRealtimeAppendAudioParams,
) -> psychevo::Result<wire::ThreadRealtimeMutationResult> {
    let session = ensure_realtime_session(state, &params.session_id)?;
    let mime_type = params
        .audio
        .mime_type
        .unwrap_or_else(|| ai_audio_format(params.audio.format).mime_type().to_string());
    let audio = Media::from_base64(mime_type, params.audio.data);
    session
        .sender
        .send_audio(audio)
        .await
        .map_err(voice_runtime_error)?;
    Ok(realtime_accepted())
}

pub(super) async fn append_realtime_text(
    state: &WebState,
    params: wire::ThreadRealtimeAppendTextParams,
) -> psychevo::Result<wire::ThreadRealtimeMutationResult> {
    let session = ensure_realtime_session(state, &params.session_id)?;
    session
        .sender
        .send_text(params.text)
        .await
        .map_err(voice_runtime_error)?;
    Ok(realtime_accepted())
}

pub(super) async fn append_realtime_speech(
    state: &WebState,
    params: wire::ThreadRealtimeAppendSpeechParams,
) -> psychevo::Result<wire::ThreadRealtimeMutationResult> {
    let session = ensure_realtime_session(state, &params.session_id)?;
    session
        .sender
        .send_text(params.text)
        .await
        .map_err(voice_runtime_error)?;
    session.sender.commit().await.map_err(voice_runtime_error)?;
    Ok(realtime_accepted())
}

pub(super) fn stop_realtime(
    state: &WebState,
    out_tx: ConnectionSender,
    params: wire::ThreadRealtimeSessionParams,
) -> psychevo::Result<wire::ThreadRealtimeMutationResult> {
    let removed = state
        .inner
        .realtime_sessions
        .lock()
        .expect("realtime sessions poisoned")
        .remove(&params.session_id);
    let accepted = removed.is_some();
    if let Some(session) = removed {
        state
            .inner
            .gateway
            .spawn_background("realtime-voice-close", async move {
                let _ = session.sender.close().await;
            });
        let _ = out_tx.send(rpc_notification(
            "thread/realtime/closed",
            json!(wire::ThreadRealtimeClosedNotification {
                session_id: params.session_id,
                reason: "requested".to_string(),
            }),
        ));
    }
    Ok(wire::ThreadRealtimeMutationResult {
        accepted,
        message: (!accepted).then(|| "unknown realtime session".to_string()),
    })
}

pub(super) fn list_realtime_voices(
    state: &WebState,
    params: wire::ThreadRealtimeSessionParams,
) -> psychevo::Result<wire::ThreadRealtimeListVoicesResult> {
    let session = ensure_realtime_session(state, &params.session_id)?;
    let voices = if session.provider == "fake" {
        vec![wire::ThreadRealtimeVoiceView {
            id: "fake".to_string(),
            label: "Fake voice".to_string(),
        }]
    } else {
        Vec::new()
    };
    Ok(wire::ThreadRealtimeListVoicesResult { voices })
}

pub(super) fn voice_policy_for_source(
    state: &WebState,
    source: &GatewaySource,
) -> wire::VoicePolicyMode {
    state
        .inner
        .voice_policies
        .lock()
        .expect("voice policies poisoned")
        .get(&source.source_key().0)
        .copied()
        .unwrap_or(wire::VoicePolicyMode::Off)
}

pub(super) fn update_voice_policy_for_source(
    state: &WebState,
    source: &GatewaySource,
    mode: wire::VoicePolicyMode,
) -> wire::VoicePolicyResult {
    let target = source.source_key().0;
    let mut policies = state
        .inner
        .voice_policies
        .lock()
        .expect("voice policies poisoned");
    if mode == wire::VoicePolicyMode::Off {
        policies.remove(&target);
    } else {
        policies.insert(target.clone(), mode);
    }
    wire::VoicePolicyResult { mode, target }
}

async fn voice_policy_target(
    state: &WebState,
    auth: &AuthContext,
    scope: Option<wire::GatewayRequestScope>,
    source_key: Option<SourceKey>,
    thread_id: Option<String>,
) -> psychevo::Result<String> {
    if let Some(thread_id) = thread_id {
        authorize_thread(state, auth, &thread_id).await?;
        return Ok(format!("thread:{thread_id}"));
    }
    if let Some(source_key) = source_key {
        return Ok(source_key.0);
    }
    let scope = resolve_optional_scope(state, auth, scope)?;
    Ok(scope.source.source_key().0)
}

fn ensure_realtime_session(
    state: &WebState,
    session_id: &str,
) -> psychevo::Result<RealtimeSessionState> {
    state
        .inner
        .realtime_sessions
        .lock()
        .expect("realtime sessions poisoned")
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown realtime session: {session_id}")))
}

fn realtime_accepted() -> wire::ThreadRealtimeMutationResult {
    wire::ThreadRealtimeMutationResult {
        accepted: true,
        message: None,
    }
}

fn realtime_event_notification(
    session_id: &str,
    _thread_id: &str,
    event: RealtimeEvent,
) -> Option<String> {
    match event {
        RealtimeEvent::InputTranscriptDelta { delta } => Some(rpc_notification(
            "thread/realtime/transcript/delta",
            json!(wire::ThreadRealtimeTranscriptNotification {
                session_id: session_id.to_string(),
                role: "user".to_string(),
                text: delta,
            }),
        )),
        RealtimeEvent::InputTranscriptDone { text } => Some(rpc_notification(
            "thread/realtime/transcript/done",
            json!(wire::ThreadRealtimeTranscriptNotification {
                session_id: session_id.to_string(),
                role: "user".to_string(),
                text,
            }),
        )),
        RealtimeEvent::OutputTextDelta { delta } => Some(rpc_notification(
            "thread/realtime/transcript/delta",
            json!(wire::ThreadRealtimeTranscriptNotification {
                session_id: session_id.to_string(),
                role: "assistant".to_string(),
                text: delta,
            }),
        )),
        RealtimeEvent::OutputTextDone { text } => Some(rpc_notification(
            "thread/realtime/transcript/done",
            json!(wire::ThreadRealtimeTranscriptNotification {
                session_id: session_id.to_string(),
                role: "assistant".to_string(),
                text,
            }),
        )),
        RealtimeEvent::OutputAudioDelta { audio } => Some(rpc_notification(
            "thread/realtime/outputAudio/delta",
            json!(wire::ThreadRealtimeOutputAudioDeltaNotification {
                session_id: session_id.to_string(),
                data: audio.base64().ok()?.to_string(),
                format: wire_audio_format_from_mime(audio.mime_type()),
            }),
        )),
        RealtimeEvent::Warning { warning } => Some(rpc_notification(
            "thread/realtime/error",
            json!(wire::ThreadRealtimeErrorNotification {
                session_id: session_id.to_string(),
                message: warning.message,
            }),
        )),
        RealtimeEvent::Closed { reason } => Some(rpc_notification(
            "thread/realtime/closed",
            json!(wire::ThreadRealtimeClosedNotification {
                session_id: session_id.to_string(),
                reason: match reason {
                    RealtimeCloseReason::Requested => "requested".to_string(),
                    RealtimeCloseReason::Remote => "remote".to_string(),
                    RealtimeCloseReason::Aborted => "aborted".to_string(),
                },
            }),
        )),
        RealtimeEvent::OutputAudioDone
        | RealtimeEvent::ResponseDone
        | RealtimeEvent::Metadata { .. } => None,
    }
}

fn voice_provider(
    provider: &str,
    base_url: &str,
    api_key: Option<&str>,
    api_key_env: &Option<String>,
) -> psychevo::Result<psychevo::__ai::Provider> {
    if provider == "fake" {
        return Provider::builder(
            DeploymentConfig::new("fake", "fake", "fake://local")
                .with_default_language_protocol("fake"),
        )
        .transcription_adapter(FakeTranscriptionAdapter::default())
        .speech_adapter(FakeSpeechAdapter::new(Media::from_base64(
            "audio/wav",
            "UklGRg==",
        )))
        .build()
        .map_err(voice_runtime_error);
    }
    if !is_xiaomi_voice_provider(provider) {
        return Err(Error::Config(format!(
            "voice provider is not supported yet: {provider}"
        )));
    }
    let api_key = api_key.ok_or_else(|| missing_voice_credentials(api_key_env))?;
    Xiaomi::builder(
        DeploymentConfig::new(provider, provider, base_url)
            .with_default_language_protocol("xiaomi_voice"),
    )
    .with_api_key(SecretValue::new(api_key))
    .build()
    .and_then(|facade| facade.provider())
    .map_err(voice_runtime_error)
}

fn is_xiaomi_voice_provider(provider: &str) -> bool {
    matches!(provider, "xiaomi" | "xiaomi-token-plan")
}

fn voice_runtime_error(err: impl std::fmt::Display) -> Error {
    Error::Message(format!("voice provider failed: {err}"))
}

fn missing_voice_credentials(api_key_env: &Option<String>) -> Error {
    Error::Config(format!(
        "missing {}",
        api_key_env
            .as_deref()
            .unwrap_or("voice provider credentials")
    ))
}

fn ai_audio_format(
    format: wire::VoiceAudioFormat,
) -> psychevo::__product::configuration::VoiceAudioFormat {
    match format {
        wire::VoiceAudioFormat::Wav => psychevo::__product::configuration::VoiceAudioFormat::Wav,
        wire::VoiceAudioFormat::Mp3 => psychevo::__product::configuration::VoiceAudioFormat::Mp3,
        wire::VoiceAudioFormat::Pcm16 => {
            psychevo::__product::configuration::VoiceAudioFormat::Pcm16
        }
    }
}

fn validated_asr_mime_type(
    format: psychevo::__product::configuration::VoiceAudioFormat,
    declared_mime_type: Option<&str>,
) -> psychevo::Result<String> {
    if !format.supports_asr_input() {
        return Err(Error::Message(format!(
            "unsupported ASR audio format `{}`",
            format.as_str()
        )));
    }
    if let Some(declared) = declared_mime_type {
        let declared = declared.trim().to_ascii_lowercase();
        let matches = match format {
            psychevo::__product::configuration::VoiceAudioFormat::Wav => {
                matches!(
                    declared.as_str(),
                    "audio/wav" | "audio/wave" | "audio/x-wav"
                )
            }
            psychevo::__product::configuration::VoiceAudioFormat::Mp3 => {
                matches!(declared.as_str(), "audio/mpeg" | "audio/mp3")
            }
            psychevo::__product::configuration::VoiceAudioFormat::Pcm16 => false,
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

fn wire_audio_format(
    format: psychevo::__product::configuration::VoiceAudioFormat,
) -> wire::VoiceAudioFormat {
    match format {
        psychevo::__product::configuration::VoiceAudioFormat::Wav => wire::VoiceAudioFormat::Wav,
        psychevo::__product::configuration::VoiceAudioFormat::Mp3 => wire::VoiceAudioFormat::Mp3,
        psychevo::__product::configuration::VoiceAudioFormat::Pcm16 => {
            wire::VoiceAudioFormat::Pcm16
        }
    }
}

fn wire_audio_format_from_mime(mime_type: &str) -> wire::VoiceAudioFormat {
    match mime_type {
        "audio/mpeg" | "audio/mp3" => wire::VoiceAudioFormat::Mp3,
        "audio/pcm" | "audio/l16" => wire::VoiceAudioFormat::Pcm16,
        _ => wire::VoiceAudioFormat::Wav,
    }
}

fn ai_realtime_transport(
    transport: wire::RealtimeTransport,
) -> psychevo::__product::configuration::VoiceRealtimeTransport {
    match transport {
        wire::RealtimeTransport::Webrtc => {
            psychevo::__product::configuration::VoiceRealtimeTransport::Webrtc
        }
        wire::RealtimeTransport::Websocket => {
            psychevo::__product::configuration::VoiceRealtimeTransport::Websocket
        }
    }
}
