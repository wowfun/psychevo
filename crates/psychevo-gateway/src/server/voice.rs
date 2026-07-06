use super::*;
use psychevo_ai::{
    AbortSignal, FakeAsrProvider, FakeRealtimeProvider, FakeTtsProvider, VoiceAsrProvider,
    VoiceAsrRequest, VoiceAudioInput as AiVoiceAudioInput, VoiceRealtimeEvent,
    VoiceRealtimeProvider, VoiceRealtimeStartRequest, VoiceTtsProvider, VoiceTtsRequest,
    XiaomiVoiceProvider,
};

#[derive(Debug, Clone)]
pub(super) struct RealtimeSessionState {
    pub(super) provider: String,
    pub(super) abort_tx: tokio::sync::watch::Sender<bool>,
}

pub(super) async fn voice_asr_transcribe_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::VoiceAsrTranscribeParams,
) -> psychevo_runtime::Result<Value> {
    let scope = resolve_optional_scope(state, auth, params.scope.clone())?;
    let options = state.run_options(scope.cwd, None);
    let resolved = resolve_voice_asr_config(
        &options,
        params.provider.as_deref(),
        params.model.as_deref(),
        params.language.as_deref(),
    )?;
    let audio_format = ai_audio_format(params.audio.format);
    let request = VoiceAsrRequest {
        provider: resolved.provider.clone(),
        model: resolved.model.clone(),
        language: resolved.language.clone(),
        audio: AiVoiceAudioInput {
            data: params.audio.data,
            format: audio_format,
            mime_type: params.audio.mime_type,
        },
    };
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let result = if resolved.provider == "fake" {
        FakeAsrProvider::new("fake transcript")
            .transcribe(request, AbortSignal::new(abort_rx))
            .await
            .map_err(voice_runtime_error)?
    } else if is_xiaomi_voice_provider(&resolved.provider) {
        let api_key = resolved
            .api_key
            .clone()
            .ok_or_else(|| missing_voice_credentials(&resolved.api_key_env))?;
        XiaomiVoiceProvider::new(resolved.base_url, api_key, resolved.provider.clone())
            .transcribe(request, AbortSignal::new(abort_rx))
            .await
            .map_err(voice_runtime_error)?
    } else {
        return Err(Error::Config(format!(
            "voice ASR provider is not supported yet: {}",
            resolved.provider
        )));
    };
    Ok(serde_json::to_value(wire::VoiceAsrTranscribeResult {
        transcript: result.transcript,
        provider: result.provider,
        model: result.model,
        language: result.language,
        metadata: Some(result.metadata),
    })?)
}

pub(super) async fn voice_tts_synthesize_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::VoiceTtsSynthesizeParams,
) -> psychevo_runtime::Result<Value> {
    let scope = resolve_optional_scope(state, auth, params.scope.clone())?;
    let options = state.run_options(scope.cwd, None);
    let resolved = resolve_voice_tts_config(
        &options,
        params.provider.as_deref(),
        params.model.as_deref(),
        params.voice.as_deref(),
        params.format.map(ai_audio_format),
    )?;
    let request = VoiceTtsRequest {
        provider: resolved.provider.clone(),
        model: resolved.model.clone(),
        voice: resolved.voice.clone(),
        format: resolved.format,
        text: params.text,
    };
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let result = if resolved.provider == "fake" {
        FakeTtsProvider::new("UklGRg==")
            .synthesize(request, AbortSignal::new(abort_rx))
            .await
            .map_err(voice_runtime_error)?
    } else if is_xiaomi_voice_provider(&resolved.provider) {
        let api_key = resolved
            .api_key
            .clone()
            .ok_or_else(|| missing_voice_credentials(&resolved.api_key_env))?;
        XiaomiVoiceProvider::new(resolved.base_url, api_key, resolved.provider.clone())
            .synthesize(request, AbortSignal::new(abort_rx))
            .await
            .map_err(voice_runtime_error)?
    } else {
        return Err(Error::Config(format!(
            "voice TTS provider is not supported yet: {}",
            resolved.provider
        )));
    };
    Ok(serde_json::to_value(wire::VoiceTtsSynthesizeResult {
        audio: wire::VoiceAudioOutput {
            data: result.audio.data,
            format: wire_audio_format(result.audio.format),
            mime_type: result.audio.mime_type,
        },
        provider: result.provider,
        model: result.model,
        voice: result.voice,
        metadata: Some(result.metadata),
    })?)
}

pub(super) fn voice_policy_read_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::VoicePolicyReadParams,
) -> psychevo_runtime::Result<Value> {
    let target = voice_policy_target(
        state,
        auth,
        params.scope,
        params.source_key,
        params.thread_id,
    )?;
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

pub(super) fn voice_policy_update_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::VoicePolicyUpdateParams,
) -> psychevo_runtime::Result<Value> {
    let target = voice_policy_target(
        state,
        auth,
        params.scope,
        params.source_key,
        params.thread_id,
    )?;
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

pub(super) async fn voice_realtime_start_value(
    state: &WebState,
    auth: &AuthContext,
    out_tx: mpsc::UnboundedSender<String>,
    params: wire::ThreadRealtimeStartParams,
) -> psychevo_runtime::Result<Value> {
    authorize_thread(state, auth, &params.thread_id)?;
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
    let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let (result, mut stream) = FakeRealtimeProvider
        .start(
            VoiceRealtimeStartRequest {
                thread_id: params.thread_id.clone(),
                provider: resolved.provider.clone(),
                model: resolved.model.clone(),
                transport: resolved.transport,
                voice: resolved.voice.clone(),
                sdp_offer: params.sdp_offer,
            },
            AbortSignal::new(abort_rx),
        )
        .await
        .map_err(voice_runtime_error)?;
    state
        .inner
        .realtime_sessions
        .lock()
        .expect("realtime sessions poisoned")
        .insert(
            result.session_id.clone(),
            RealtimeSessionState {
                provider: resolved.provider,
                abort_tx,
            },
        );
    let session_id = result.session_id.clone();
    let state_for_close = state.clone();
    tokio::spawn(async move {
        while let Some(event) = stream.next().await {
            let Ok(event) = event else {
                continue;
            };
            let should_send = match &event {
                VoiceRealtimeEvent::Closed { session_id, .. } => state_for_close
                    .inner
                    .realtime_sessions
                    .lock()
                    .expect("realtime sessions poisoned")
                    .contains_key(session_id),
                _ => true,
            };
            if !should_send {
                continue;
            }
            if let Some(notification) = realtime_event_notification(event) {
                let _ = out_tx.send(notification);
            }
        }
        state_for_close
            .inner
            .realtime_sessions
            .lock()
            .expect("realtime sessions poisoned")
            .remove(&session_id);
    });
    Ok(serde_json::to_value(wire::ThreadRealtimeStartResult {
        accepted: true,
        session_id: result.session_id,
        thread_id: result.thread_id,
    })?)
}

pub(super) fn voice_realtime_append_audio_value(
    state: &WebState,
    params: wire::ThreadRealtimeAppendAudioParams,
) -> psychevo_runtime::Result<Value> {
    ensure_realtime_session(state, &params.session_id)?;
    Ok(realtime_accepted())
}

pub(super) fn voice_realtime_append_text_value(
    state: &WebState,
    params: wire::ThreadRealtimeAppendTextParams,
) -> psychevo_runtime::Result<Value> {
    ensure_realtime_session(state, &params.session_id)?;
    Ok(realtime_accepted())
}

pub(super) fn voice_realtime_append_speech_value(
    state: &WebState,
    params: wire::ThreadRealtimeAppendSpeechParams,
) -> psychevo_runtime::Result<Value> {
    ensure_realtime_session(state, &params.session_id)?;
    Ok(realtime_accepted())
}

pub(super) fn voice_realtime_stop_value(
    state: &WebState,
    out_tx: mpsc::UnboundedSender<String>,
    params: wire::ThreadRealtimeSessionParams,
) -> psychevo_runtime::Result<Value> {
    let removed = state
        .inner
        .realtime_sessions
        .lock()
        .expect("realtime sessions poisoned")
        .remove(&params.session_id);
    let accepted = removed.is_some();
    if let Some(session) = removed {
        let _ = session.abort_tx.send(true);
        let _ = out_tx.send(rpc_notification(
            "thread/realtime/closed",
            json!(wire::ThreadRealtimeClosedNotification {
                session_id: params.session_id,
                reason: "requested".to_string(),
            }),
        ));
    }
    Ok(serde_json::to_value(wire::ThreadRealtimeMutationResult {
        accepted,
        message: (!accepted).then(|| "unknown realtime session".to_string()),
    })?)
}

pub(super) fn voice_realtime_list_voices_value(
    state: &WebState,
    params: wire::ThreadRealtimeSessionParams,
) -> psychevo_runtime::Result<Value> {
    let session = ensure_realtime_session(state, &params.session_id)?;
    let voices = if session.provider == "fake" {
        vec![wire::ThreadRealtimeVoiceView {
            id: "fake".to_string(),
            label: "Fake voice".to_string(),
        }]
    } else {
        Vec::new()
    };
    Ok(serde_json::to_value(
        wire::ThreadRealtimeListVoicesResult { voices },
    )?)
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

fn voice_policy_target(
    state: &WebState,
    auth: &AuthContext,
    scope: Option<wire::GatewayRequestScope>,
    source_key: Option<SourceKey>,
    thread_id: Option<String>,
) -> psychevo_runtime::Result<String> {
    if let Some(thread_id) = thread_id {
        authorize_thread(state, auth, &thread_id)?;
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
) -> psychevo_runtime::Result<RealtimeSessionState> {
    state
        .inner
        .realtime_sessions
        .lock()
        .expect("realtime sessions poisoned")
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown realtime session: {session_id}")))
}

fn realtime_accepted() -> Value {
    serde_json::to_value(wire::ThreadRealtimeMutationResult {
        accepted: true,
        message: None,
    })
    .expect("realtime mutation result serializes")
}

fn realtime_event_notification(event: VoiceRealtimeEvent) -> Option<String> {
    match event {
        VoiceRealtimeEvent::Started {
            session_id,
            thread_id,
        } => Some(rpc_notification(
            "thread/realtime/started",
            json!(wire::ThreadRealtimeStartedNotification {
                session_id,
                thread_id,
            }),
        )),
        VoiceRealtimeEvent::Sdp { session_id, sdp } => Some(rpc_notification(
            "thread/realtime/sdp",
            json!(wire::ThreadRealtimeSdpNotification { session_id, sdp }),
        )),
        VoiceRealtimeEvent::TranscriptDelta {
            session_id,
            role,
            text,
        } => Some(rpc_notification(
            "thread/realtime/transcript/delta",
            json!(wire::ThreadRealtimeTranscriptNotification {
                session_id,
                role,
                text,
            }),
        )),
        VoiceRealtimeEvent::TranscriptDone {
            session_id,
            role,
            text,
        } => Some(rpc_notification(
            "thread/realtime/transcript/done",
            json!(wire::ThreadRealtimeTranscriptNotification {
                session_id,
                role,
                text,
            }),
        )),
        VoiceRealtimeEvent::OutputAudioDelta {
            session_id,
            data,
            format,
        } => Some(rpc_notification(
            "thread/realtime/outputAudio/delta",
            json!(wire::ThreadRealtimeOutputAudioDeltaNotification {
                session_id,
                data,
                format: wire_audio_format(format),
            }),
        )),
        VoiceRealtimeEvent::Error {
            session_id,
            message,
        } => Some(rpc_notification(
            "thread/realtime/error",
            json!(wire::ThreadRealtimeErrorNotification {
                session_id,
                message,
            }),
        )),
        VoiceRealtimeEvent::Closed { session_id, reason } => Some(rpc_notification(
            "thread/realtime/closed",
            json!(wire::ThreadRealtimeClosedNotification { session_id, reason }),
        )),
    }
}

fn is_xiaomi_voice_provider(provider: &str) -> bool {
    matches!(provider, "xiaomi" | "xiaomi-token-plan")
}

fn voice_runtime_error(err: psychevo_ai::Error) -> Error {
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

fn ai_audio_format(format: wire::VoiceAudioFormat) -> psychevo_ai::VoiceAudioFormat {
    match format {
        wire::VoiceAudioFormat::Wav => psychevo_ai::VoiceAudioFormat::Wav,
        wire::VoiceAudioFormat::Mp3 => psychevo_ai::VoiceAudioFormat::Mp3,
        wire::VoiceAudioFormat::Pcm16 => psychevo_ai::VoiceAudioFormat::Pcm16,
    }
}

fn wire_audio_format(format: psychevo_ai::VoiceAudioFormat) -> wire::VoiceAudioFormat {
    match format {
        psychevo_ai::VoiceAudioFormat::Wav => wire::VoiceAudioFormat::Wav,
        psychevo_ai::VoiceAudioFormat::Mp3 => wire::VoiceAudioFormat::Mp3,
        psychevo_ai::VoiceAudioFormat::Pcm16 => wire::VoiceAudioFormat::Pcm16,
    }
}

fn ai_realtime_transport(
    transport: wire::RealtimeTransport,
) -> psychevo_ai::VoiceRealtimeTransport {
    match transport {
        wire::RealtimeTransport::Webrtc => psychevo_ai::VoiceRealtimeTransport::Webrtc,
        wire::RealtimeTransport::Websocket => psychevo_ai::VoiceRealtimeTransport::Websocket,
    }
}
