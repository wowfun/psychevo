use psychevo::application::{
    Configuration, ConfigurationQuery, VoiceAudioFormat, VoiceAudioInput, VoiceRealtimeCloseReason,
    VoiceRealtimeConnection, VoiceRealtimeControl, VoiceRealtimeEvent, VoiceRealtimeRequest,
    VoiceSpeechRequest, VoiceTranscriptionRequest,
};
use psychevo::{Error, Result};
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};

use super::auth_input::authorize_thread;
use super::binding::{AuthContext, WebState};
use super::event_delivery::ConnectionSender;
use super::rpc_json::rpc_notification;
use super::scope_session::resolve_optional_scope;
use psychevo_gateway_protocol::source::{GatewaySource, SourceKey};

#[derive(Debug, Clone)]
pub(super) struct RealtimeSessionState {
    pub(super) control: VoiceRealtimeControl,
}

pub(super) async fn voice_asr_transcribe_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::voice::VoiceAsrTranscribeParams,
) -> Result<Value> {
    let scope = resolve_optional_scope(state, auth, params.scope)?;
    let result = voice_configuration(state, scope.cwd)?
        .transcribe_voice(VoiceTranscriptionRequest {
            audio: voice_audio_input(params.audio),
            provider: params.provider,
            model: params.model,
            language: params.language,
        })
        .await?;
    Ok(serde_json::to_value(
        wire::voice::VoiceAsrTranscribeResult {
            transcript: result.transcript,
            provider: result.provider,
            model: result.model,
            language: result.language,
            metadata: Some(Value::Object(result.metadata.into_iter().collect())),
        },
    )?)
}

pub(super) async fn voice_tts_synthesize_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::voice::VoiceTtsSynthesizeParams,
) -> Result<Value> {
    let scope = resolve_optional_scope(state, auth, params.scope)?;
    let result = voice_configuration(state, scope.cwd)?
        .synthesize_voice(VoiceSpeechRequest {
            text: params.text,
            provider: params.provider,
            model: params.model,
            voice: params.voice,
            format: params.format.map(application_voice_format),
        })
        .await?;
    Ok(serde_json::to_value(
        wire::voice::VoiceTtsSynthesizeResult {
            audio: wire::voice::VoiceAudioOutput {
                data: result.audio.data,
                format: wire_audio_format(result.audio.format),
                mime_type: result.audio.mime_type,
            },
            provider: result.provider,
            model: result.model,
            voice: result.voice,
            metadata: Some(Value::Object(result.metadata.into_iter().collect())),
        },
    )?)
}

pub(super) async fn voice_policy_read_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::voice::VoicePolicyReadParams,
) -> Result<Value> {
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
        .unwrap_or(wire::voice::VoicePolicyMode::Off);
    Ok(serde_json::to_value(wire::voice::VoicePolicyResult {
        mode,
        target,
    })?)
}

pub(super) async fn voice_policy_update_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::voice::VoicePolicyUpdateParams,
) -> Result<Value> {
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
    if params.mode == wire::voice::VoicePolicyMode::Off {
        policies.remove(&target);
    } else {
        policies.insert(target.clone(), params.mode);
    }
    Ok(serde_json::to_value(wire::voice::VoicePolicyResult {
        mode: params.mode,
        target,
    })?)
}

pub(super) async fn start_realtime(
    state: &WebState,
    auth: &AuthContext,
    out_tx: ConnectionSender,
    params: wire::voice::ThreadRealtimeStartParams,
) -> Result<wire::voice::ThreadRealtimeStartResult> {
    authorize_thread(state, auth, &params.thread_id).await?;
    let scope = resolve_optional_scope(state, auth, params.scope)?;
    let thread_id = params.thread_id;
    let connection = voice_configuration(state, scope.cwd)?
        .connect_realtime_voice(VoiceRealtimeRequest {
            thread_id: thread_id.clone(),
            provider: params.provider,
            model: params.model,
            transport: params.transport.map(application_realtime_transport),
            voice: params.voice,
            sdp_offer: params.sdp_offer,
        })
        .await?;
    let VoiceRealtimeConnection {
        provider,
        control,
        mut events,
    } = connection;
    let session_id = format!("{provider}-realtime-{thread_id}");
    state
        .inner
        .realtime_sessions
        .lock()
        .expect("realtime sessions poisoned")
        .insert(session_id.clone(), RealtimeSessionState { control });
    let _ = out_tx.send(rpc_notification(
        "thread/realtime/started",
        json!(wire::voice::ThreadRealtimeStartedNotification {
            session_id: session_id.clone(),
            thread_id: thread_id.clone(),
        }),
    ));
    let state_for_close = state.clone();
    let event_session_id = session_id.clone();
    let cleanup_session_id = session_id.clone();
    let supervisor = state.inner.gateway.clone();
    supervisor.spawn_background(format!("realtime-voice:{session_id}"), async move {
        while let Some(event) = events.next_event().await {
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    let _ = out_tx.send(rpc_notification(
                        "thread/realtime/error",
                        json!(wire::voice::ThreadRealtimeErrorNotification {
                            session_id: event_session_id.clone(),
                            message: error.to_string(),
                        }),
                    ));
                    continue;
                }
            };
            let should_send = match &event {
                VoiceRealtimeEvent::Closed { .. } => state_for_close
                    .inner
                    .realtime_sessions
                    .lock()
                    .expect("realtime sessions poisoned")
                    .contains_key(&event_session_id),
                _ => true,
            };
            if should_send
                && let Some(notification) = realtime_event_notification(&event_session_id, event)
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
    Ok(wire::voice::ThreadRealtimeStartResult {
        accepted: true,
        session_id,
        thread_id,
    })
}

pub(super) async fn append_realtime_audio(
    state: &WebState,
    params: wire::voice::ThreadRealtimeAppendAudioParams,
) -> Result<wire::voice::ThreadRealtimeMutationResult> {
    ensure_realtime_session(state, &params.session_id)?
        .control
        .append_audio(voice_audio_input(params.audio))
        .await?;
    Ok(realtime_accepted())
}

pub(super) async fn append_realtime_text(
    state: &WebState,
    params: wire::voice::ThreadRealtimeAppendTextParams,
) -> Result<wire::voice::ThreadRealtimeMutationResult> {
    ensure_realtime_session(state, &params.session_id)?
        .control
        .append_text(params.text)
        .await?;
    Ok(realtime_accepted())
}

pub(super) async fn append_realtime_speech(
    state: &WebState,
    params: wire::voice::ThreadRealtimeAppendSpeechParams,
) -> Result<wire::voice::ThreadRealtimeMutationResult> {
    ensure_realtime_session(state, &params.session_id)?
        .control
        .append_speech(params.text)
        .await?;
    Ok(realtime_accepted())
}

pub(super) fn stop_realtime(
    state: &WebState,
    out_tx: ConnectionSender,
    params: wire::voice::ThreadRealtimeSessionParams,
) -> Result<wire::voice::ThreadRealtimeMutationResult> {
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
                let _ = session.control.close().await;
            });
        let _ = out_tx.send(rpc_notification(
            "thread/realtime/closed",
            json!(wire::voice::ThreadRealtimeClosedNotification {
                session_id: params.session_id,
                reason: "requested".to_string(),
            }),
        ));
    }
    Ok(wire::voice::ThreadRealtimeMutationResult {
        accepted,
        message: (!accepted).then(|| "unknown realtime session".to_string()),
    })
}

pub(super) fn list_realtime_voices(
    state: &WebState,
    params: wire::voice::ThreadRealtimeSessionParams,
) -> Result<wire::voice::ThreadRealtimeListVoicesResult> {
    let session = ensure_realtime_session(state, &params.session_id)?;
    Ok(wire::voice::ThreadRealtimeListVoicesResult {
        voices: session
            .control
            .voices()
            .into_iter()
            .map(|voice| wire::voice::ThreadRealtimeVoiceView {
                id: voice.id,
                label: voice.label,
            })
            .collect(),
    })
}

pub(super) fn voice_policy_for_source(
    state: &WebState,
    source: &GatewaySource,
) -> wire::voice::VoicePolicyMode {
    state
        .inner
        .voice_policies
        .lock()
        .expect("voice policies poisoned")
        .get(&source.source_key().0)
        .copied()
        .unwrap_or(wire::voice::VoicePolicyMode::Off)
}

pub(super) fn update_voice_policy_for_source(
    state: &WebState,
    source: &GatewaySource,
    mode: wire::voice::VoicePolicyMode,
) -> wire::voice::VoicePolicyResult {
    let target = source.source_key().0;
    let mut policies = state
        .inner
        .voice_policies
        .lock()
        .expect("voice policies poisoned");
    if mode == wire::voice::VoicePolicyMode::Off {
        policies.remove(&target);
    } else {
        policies.insert(target.clone(), mode);
    }
    wire::voice::VoicePolicyResult { mode, target }
}

async fn voice_policy_target(
    state: &WebState,
    auth: &AuthContext,
    scope: Option<wire::source::GatewayRequestScope>,
    source_key: Option<SourceKey>,
    thread_id: Option<String>,
) -> Result<String> {
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

fn voice_configuration(state: &WebState, cwd: std::path::PathBuf) -> Result<Configuration> {
    let mut query = ConfigurationQuery::new(cwd);
    query.inherited_env = Some(state.inner.inherited_env.clone());
    state.inner.framework.configuration(query)
}

fn ensure_realtime_session(state: &WebState, session_id: &str) -> Result<RealtimeSessionState> {
    state
        .inner
        .realtime_sessions
        .lock()
        .expect("realtime sessions poisoned")
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown realtime session: {session_id}")))
}

fn realtime_accepted() -> wire::voice::ThreadRealtimeMutationResult {
    wire::voice::ThreadRealtimeMutationResult {
        accepted: true,
        message: None,
    }
}

fn realtime_event_notification(session_id: &str, event: VoiceRealtimeEvent) -> Option<String> {
    match event {
        VoiceRealtimeEvent::InputTranscriptDelta { delta } => Some(rpc_notification(
            "thread/realtime/transcript/delta",
            json!(wire::voice::ThreadRealtimeTranscriptNotification {
                session_id: session_id.to_string(),
                role: "user".to_string(),
                text: delta,
            }),
        )),
        VoiceRealtimeEvent::InputTranscriptDone { text } => Some(rpc_notification(
            "thread/realtime/transcript/done",
            json!(wire::voice::ThreadRealtimeTranscriptNotification {
                session_id: session_id.to_string(),
                role: "user".to_string(),
                text,
            }),
        )),
        VoiceRealtimeEvent::OutputTextDelta { delta } => Some(rpc_notification(
            "thread/realtime/transcript/delta",
            json!(wire::voice::ThreadRealtimeTranscriptNotification {
                session_id: session_id.to_string(),
                role: "assistant".to_string(),
                text: delta,
            }),
        )),
        VoiceRealtimeEvent::OutputTextDone { text } => Some(rpc_notification(
            "thread/realtime/transcript/done",
            json!(wire::voice::ThreadRealtimeTranscriptNotification {
                session_id: session_id.to_string(),
                role: "assistant".to_string(),
                text,
            }),
        )),
        VoiceRealtimeEvent::OutputAudioDelta { audio } => Some(rpc_notification(
            "thread/realtime/outputAudio/delta",
            json!(wire::voice::ThreadRealtimeOutputAudioDeltaNotification {
                session_id: session_id.to_string(),
                data: audio.data,
                format: wire_audio_format(audio.format),
            }),
        )),
        VoiceRealtimeEvent::Warning { message } => Some(rpc_notification(
            "thread/realtime/error",
            json!(wire::voice::ThreadRealtimeErrorNotification {
                session_id: session_id.to_string(),
                message,
            }),
        )),
        VoiceRealtimeEvent::Closed { reason } => Some(rpc_notification(
            "thread/realtime/closed",
            json!(wire::voice::ThreadRealtimeClosedNotification {
                session_id: session_id.to_string(),
                reason: match reason {
                    VoiceRealtimeCloseReason::Requested => "requested".to_string(),
                    VoiceRealtimeCloseReason::Remote => "remote".to_string(),
                    VoiceRealtimeCloseReason::Aborted => "aborted".to_string(),
                },
            }),
        )),
        VoiceRealtimeEvent::OutputAudioDone
        | VoiceRealtimeEvent::ResponseDone
        | VoiceRealtimeEvent::Metadata { .. } => None,
    }
}

fn voice_audio_input(audio: wire::voice::VoiceAudioInput) -> VoiceAudioInput {
    VoiceAudioInput {
        data: audio.data,
        format: application_voice_format(audio.format),
        mime_type: audio.mime_type,
    }
}

fn application_voice_format(format: wire::voice::VoiceAudioFormat) -> VoiceAudioFormat {
    match format {
        wire::voice::VoiceAudioFormat::Wav => VoiceAudioFormat::Wav,
        wire::voice::VoiceAudioFormat::Mp3 => VoiceAudioFormat::Mp3,
        wire::voice::VoiceAudioFormat::Pcm16 => VoiceAudioFormat::Pcm16,
    }
}

fn wire_audio_format(format: VoiceAudioFormat) -> wire::voice::VoiceAudioFormat {
    match format {
        VoiceAudioFormat::Wav => wire::voice::VoiceAudioFormat::Wav,
        VoiceAudioFormat::Mp3 => wire::voice::VoiceAudioFormat::Mp3,
        VoiceAudioFormat::Pcm16 => wire::voice::VoiceAudioFormat::Pcm16,
    }
}

fn application_realtime_transport(
    transport: wire::voice::RealtimeTransport,
) -> psychevo::application::VoiceRealtimeTransport {
    match transport {
        wire::voice::RealtimeTransport::Webrtc => {
            psychevo::application::VoiceRealtimeTransport::Webrtc
        }
        wire::voice::RealtimeTransport::Websocket => {
            psychevo::application::VoiceRealtimeTransport::Websocket
        }
    }
}
