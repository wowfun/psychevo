use psychevo_ai::{
    AbortSignal, FakeAsrProvider, FakeRealtimeProvider, FakeTtsProvider, VoiceAsrProvider,
    VoiceAsrRequest, VoiceAudioFormat, VoiceAudioInput, VoiceRealtimeProvider, VoiceTtsProvider,
    VoiceTtsRequest, parse_xiaomi_asr_response, parse_xiaomi_tts_response,
    parse_xiaomi_tts_sse_audio_delta, validate_voice_asr_request, xiaomi_asr_request_body,
    xiaomi_tts_request_body,
};
use serde_json::json;

#[tokio::test]
async fn fake_asr_transcribes_without_network() {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    let provider = FakeAsrProvider::new("hello from mic");
    let result = provider
        .transcribe(asr_request(), AbortSignal::new(rx))
        .await
        .expect("fake asr");

    assert_eq!(result.transcript, "hello from mic");
    assert_eq!(result.provider, "fake");
}

#[tokio::test]
async fn fake_tts_returns_requested_format() {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    let provider = FakeTtsProvider::new("UklGRg==");
    let result = provider
        .synthesize(tts_request(VoiceAudioFormat::Wav), AbortSignal::new(rx))
        .await
        .expect("fake tts");

    assert_eq!(result.audio.data, "UklGRg==");
    assert_eq!(result.audio.mime_type, "audio/wav");
}

#[tokio::test]
async fn fake_realtime_emits_started_transcript_and_closed() {
    let (tx, rx) = tokio::sync::watch::channel(false);
    let provider = FakeRealtimeProvider;
    let (result, mut events) = provider
        .start(
            psychevo_ai::VoiceRealtimeStartRequest {
                thread_id: "thread-1".to_string(),
                provider: "fake".to_string(),
                model: "fake-realtime".to_string(),
                transport: psychevo_ai::VoiceRealtimeTransport::Websocket,
                voice: None,
                sdp_offer: None,
            },
            AbortSignal::new(rx),
        )
        .await
        .expect("fake realtime");

    assert_eq!(result.session_id, "fake-realtime-thread-1");
    let started = futures::StreamExt::next(&mut events)
        .await
        .expect("started")
        .expect("started event");
    assert!(matches!(
        started,
        psychevo_ai::VoiceRealtimeEvent::Started { .. }
    ));
    let transcript = futures::StreamExt::next(&mut events)
        .await
        .expect("transcript")
        .expect("transcript event");
    assert!(matches!(
        transcript,
        psychevo_ai::VoiceRealtimeEvent::TranscriptDone { .. }
    ));
    tx.send(true).expect("abort");
    let closed = futures::StreamExt::next(&mut events)
        .await
        .expect("closed")
        .expect("closed event");
    assert!(matches!(
        closed,
        psychevo_ai::VoiceRealtimeEvent::Closed { .. }
    ));
}

#[test]
fn validates_asr_format_and_encoded_size() {
    let mut request = asr_request();
    request.audio.format = VoiceAudioFormat::Pcm16;
    assert!(validate_voice_asr_request(&request).is_err());

    request.audio.format = VoiceAudioFormat::Wav;
    request.audio.data = "x".repeat(10 * 1024 * 1024 + 1);
    assert!(validate_voice_asr_request(&request).is_err());
}

#[test]
fn xiaomi_asr_body_uses_input_audio_and_language() {
    let body = xiaomi_asr_request_body(&asr_request());

    assert_eq!(body["model"], "mimo-v2.5-asr");
    assert_eq!(body["messages"][0]["content"][0]["type"], "input_audio");
    assert_eq!(
        body["messages"][0]["content"][0]["input_audio"]["format"],
        "wav"
    );
    assert_eq!(body["asr_options"]["language"], "auto");
}

#[test]
fn xiaomi_tts_body_uses_audio_options() {
    let body = xiaomi_tts_request_body(&tts_request(VoiceAudioFormat::Wav), false);

    assert_eq!(body["model"], "mimo-v2.5-tts");
    assert_eq!(body["audio"]["format"], "wav");
    assert_eq!(body["audio"]["voice"], "mimo_default");
    assert_eq!(body["stream"], false);
}

#[test]
fn parses_flexible_xiaomi_asr_and_tts_shapes() {
    let asr = parse_xiaomi_asr_response(&json!({
        "choices": [{"message": {"content": [{"type": "text", "text": "hello"}]}}]
    }))
    .expect("asr parse");
    assert_eq!(asr, "hello");

    let tts = parse_xiaomi_tts_response(
        &json!({"choices": [{"message": {"audio": {"data": "pcm-data"}}}]}),
        VoiceAudioFormat::Pcm16,
    )
    .expect("tts parse");
    assert_eq!(tts.data, "pcm-data");
    assert_eq!(tts.mime_type, "audio/pcm");
}

#[test]
fn parses_xiaomi_tts_sse_audio_delta() {
    let chunk = parse_xiaomi_tts_sse_audio_delta(&json!({
        "choices": [{
            "delta": {
                "audio": {
                    "data": "chunk",
                    "format": "pcm16",
                    "sample_rate": 24000
                }
            }
        }]
    }))
    .expect("audio chunk");

    assert_eq!(chunk.data, "chunk");
    assert_eq!(chunk.format, VoiceAudioFormat::Pcm16);
    assert_eq!(chunk.sample_rate, Some(24000));
}

fn asr_request() -> VoiceAsrRequest {
    VoiceAsrRequest {
        provider: "fake".to_string(),
        model: "mimo-v2.5-asr".to_string(),
        language: Some("auto".to_string()),
        audio: VoiceAudioInput {
            data: "UklGRg==".to_string(),
            format: VoiceAudioFormat::Wav,
            mime_type: Some("audio/wav".to_string()),
        },
    }
}

fn tts_request(format: VoiceAudioFormat) -> VoiceTtsRequest {
    VoiceTtsRequest {
        provider: "fake".to_string(),
        model: "mimo-v2.5-tts".to_string(),
        voice: "mimo_default".to_string(),
        format,
        text: "hello".to_string(),
    }
}
