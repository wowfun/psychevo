#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
fn voice_config_parses_documented_blocks() {
    let config = crate::config::config_parse::parse_run_config(json!({
        "voice": {
            "asr": {
                "provider": "xiaomi-token-plan",
                "model": "mimo-v2.5-asr",
                "language": "auto"
            },
            "tts": {
                "provider": "xiaomi-token-plan",
                "model": "mimo-v2.5-tts",
                "voice": "mimo_default",
                "format": "wav"
            },
            "realtime": {
                "provider": "openai",
                "model": "gpt-realtime-2",
                "transport": "webrtc",
                "voice": "marin"
            }
        }
    }))
    .expect("voice config");

    assert_eq!(config.voice.asr.provider, "xiaomi-token-plan");
    assert_eq!(config.voice.tts.model, "mimo-v2.5-tts");
    assert_eq!(
        config
            .voice
            .realtime
            .as_ref()
            .map(|config| config.model.as_str()),
        Some("gpt-realtime-2")
    );
}

#[test]
fn voice_config_rejects_raw_keys_and_invalid_formats() {
    let raw_key = crate::config::config_parse::parse_run_config(json!({
        "voice": {
            "asr": {
                "api_key": "secret"
            }
        }
    }))
    .expect_err("raw key");
    assert!(
        raw_key
            .to_string()
            .contains("must not contain raw API keys")
    );

    let invalid_format = crate::config::config_parse::parse_run_config(json!({
        "voice": {
            "tts": {
                "format": "mp3"
            }
        }
    }))
    .expect_err("invalid tts format");
    assert!(invalid_format.to_string().contains("voice.tts.format"));

    let invalid_transport = crate::config::config_parse::parse_run_config(json!({
        "voice": {
            "realtime": {
                "provider": "openai",
                "model": "gpt-realtime-2",
                "transport": "smtp"
            }
        }
    }))
    .expect_err("invalid transport");
    assert!(
        invalid_transport
            .to_string()
            .contains("voice.realtime.transport")
    );
}

#[test]
fn fake_voice_provider_resolves_without_credentials() {
    let temp = tempdir().expect("temp");
    fs::create_dir_all(home_dir(&temp)).expect("home");
    fs::write(
        home_dir(&temp).join("config.toml"),
        r#"
[voice.asr]
provider = "fake"
model = "fake-asr"

[voice.tts]
provider = "fake"
model = "fake-tts"
voice = "test"
format = "wav"

[voice.realtime]
provider = "fake"
model = "fake-realtime"
transport = "websocket"
"#,
    )
    .expect("config");
    let options = base_options(&temp);

    let asr = crate::config::resolve_voice_asr_config(&options, None, None, None).expect("asr");
    assert_eq!(asr.provider, "fake");
    assert_eq!(asr.api_key, None);

    let tts =
        crate::config::resolve_voice_tts_config(&options, None, None, None, None).expect("tts");
    assert_eq!(tts.voice, "test");

    let realtime = crate::config::resolve_voice_realtime_config(&options, None, None, None, None)
        .expect("realtime")
        .expect("configured");
    assert_eq!(realtime.provider, "fake");
    assert_eq!(
        realtime.transport,
        psychevo_ai::VoiceRealtimeTransport::Websocket
    );
}
