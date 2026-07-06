#[allow(unused_imports)]
pub(crate) use super::*;

pub const MAX_VOICE_AUDIO_BASE64_BYTES: usize = 10 * 1024 * 1024;

pub type VoiceAudioStream = BoxStream<'static, Result<VoiceAudioChunk>>;
pub type VoiceRealtimeStream = BoxStream<'static, Result<VoiceRealtimeEvent>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceAudioFormat {
    Wav,
    Mp3,
    Pcm16,
}

impl VoiceAudioFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Pcm16 => "pcm16",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::Mp3 => "audio/mpeg",
            Self::Pcm16 => "audio/pcm",
        }
    }

    pub fn supports_asr_input(self) -> bool {
        matches!(self, Self::Wav | Self::Mp3)
    }

    pub fn supports_tts_output(self) -> bool {
        matches!(self, Self::Wav | Self::Pcm16)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceAudioInput {
    pub data: String,
    pub format: VoiceAudioFormat,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceAudioOutput {
    pub data: String,
    pub format: VoiceAudioFormat,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceAudioChunk {
    pub data: String,
    pub format: VoiceAudioFormat,
    pub sample_rate: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceAsrRequest {
    pub provider: String,
    pub model: String,
    pub language: Option<String>,
    pub audio: VoiceAudioInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceAsrResult {
    pub transcript: String,
    pub provider: String,
    pub model: String,
    pub language: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceTtsRequest {
    pub provider: String,
    pub model: String,
    pub voice: String,
    pub format: VoiceAudioFormat,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceTtsResult {
    pub audio: VoiceAudioOutput,
    pub provider: String,
    pub model: String,
    pub voice: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceRealtimeTransport {
    Webrtc,
    Websocket,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceRealtimeStartRequest {
    pub thread_id: String,
    pub provider: String,
    pub model: String,
    pub transport: VoiceRealtimeTransport,
    pub voice: Option<String>,
    pub sdp_offer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceRealtimeStartResult {
    pub session_id: String,
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VoiceRealtimeEvent {
    Started {
        session_id: String,
        thread_id: String,
    },
    Sdp {
        session_id: String,
        sdp: String,
    },
    TranscriptDelta {
        session_id: String,
        role: String,
        text: String,
    },
    TranscriptDone {
        session_id: String,
        role: String,
        text: String,
    },
    OutputAudioDelta {
        session_id: String,
        data: String,
        format: VoiceAudioFormat,
    },
    Error {
        session_id: String,
        message: String,
    },
    Closed {
        session_id: String,
        reason: String,
    },
}

pub trait VoiceAsrProvider: Send + Sync {
    fn transcribe(
        &self,
        request: VoiceAsrRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<VoiceAsrResult>>;
}

pub trait VoiceTtsProvider: Send + Sync {
    fn synthesize(
        &self,
        request: VoiceTtsRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<VoiceTtsResult>>;
}

pub trait VoiceRealtimeProvider: Send + Sync {
    fn start(
        &self,
        request: VoiceRealtimeStartRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<(VoiceRealtimeStartResult, VoiceRealtimeStream)>>;
}

#[derive(Debug, Clone)]
pub struct FakeAsrProvider {
    transcript: String,
}

impl FakeAsrProvider {
    pub fn new(transcript: impl Into<String>) -> Self {
        Self {
            transcript: transcript.into(),
        }
    }
}

impl VoiceAsrProvider for FakeAsrProvider {
    fn transcribe(
        &self,
        request: VoiceAsrRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<VoiceAsrResult>> {
        let transcript = self.transcript.clone();
        Box::pin(async move {
            if abort.aborted() {
                return Err(Error::Provider("ASR request aborted".to_string()));
            }
            Ok(VoiceAsrResult {
                transcript,
                provider: request.provider,
                model: request.model,
                language: request.language,
                metadata: json!({"provider": "fake"}),
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct FakeTtsProvider {
    audio_data: String,
}

impl FakeTtsProvider {
    pub fn new(audio_data: impl Into<String>) -> Self {
        Self {
            audio_data: audio_data.into(),
        }
    }
}

impl VoiceTtsProvider for FakeTtsProvider {
    fn synthesize(
        &self,
        request: VoiceTtsRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<VoiceTtsResult>> {
        let audio_data = self.audio_data.clone();
        Box::pin(async move {
            if abort.aborted() {
                return Err(Error::Provider("TTS request aborted".to_string()));
            }
            Ok(VoiceTtsResult {
                audio: VoiceAudioOutput {
                    data: audio_data,
                    format: request.format,
                    mime_type: request.format.mime_type().to_string(),
                },
                provider: request.provider,
                model: request.model,
                voice: request.voice,
                metadata: json!({"provider": "fake"}),
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct FakeRealtimeProvider;

impl VoiceRealtimeProvider for FakeRealtimeProvider {
    fn start(
        &self,
        request: VoiceRealtimeStartRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<(VoiceRealtimeStartResult, VoiceRealtimeStream)>> {
        Box::pin(async move {
            if abort.aborted() {
                return Err(Error::Provider("realtime request aborted".to_string()));
            }
            let session_id = format!("fake-realtime-{}", request.thread_id);
            let result = VoiceRealtimeStartResult {
                session_id: session_id.clone(),
                thread_id: request.thread_id.clone(),
            };
            let thread_id = request.thread_id;
            let events = stream::unfold(
                (0_u8, abort, session_id, thread_id),
                |(step, mut abort, session_id, thread_id)| async move {
                    match step {
                        0 => Some((
                            Ok(VoiceRealtimeEvent::Started {
                                session_id: session_id.clone(),
                                thread_id: thread_id.clone(),
                            }),
                            (1, abort, session_id, thread_id),
                        )),
                        1 => Some((
                            Ok(VoiceRealtimeEvent::TranscriptDone {
                                session_id: session_id.clone(),
                                role: "user".to_string(),
                                text: "fake realtime transcript".to_string(),
                            }),
                            (2, abort, session_id, thread_id),
                        )),
                        2 => {
                            abort.wait_for_abort().await;
                            Some((
                                Ok(VoiceRealtimeEvent::Closed {
                                    session_id: session_id.clone(),
                                    reason: "requested".to_string(),
                                }),
                                (3, abort, session_id, thread_id),
                            ))
                        }
                        _ => None,
                    }
                },
            );
            Ok((result, Box::pin(events) as Pin<Box<_>>))
        })
    }
}

#[derive(Debug, Clone)]
pub struct XiaomiVoiceProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    provider_name: String,
}

impl XiaomiVoiceProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        provider_name: impl Into<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            provider_name: provider_name.into(),
        }
    }

    #[cfg(test)]
    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }
}

impl VoiceAsrProvider for XiaomiVoiceProvider {
    fn transcribe(
        &self,
        request: VoiceAsrRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<VoiceAsrResult>> {
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let api_key = self.api_key.clone();
        let provider_name = self.provider_name.clone();
        Box::pin(async move {
            let mut abort = abort;
            validate_voice_asr_request(&request)?;
            let endpoint = openai_chat_completions_endpoint(&base_url);
            let mut http_request = client
                .post(endpoint)
                .header("accept", "application/json")
                .json(&xiaomi_asr_request_body(&request));
            if !api_key.trim().is_empty() {
                http_request = http_request.bearer_auth(api_key);
            }
            let response = tokio::select! {
                biased;
                _ = abort.wait_for_abort() => {
                    return Err(Error::Provider("ASR request aborted".to_string()));
                }
                response = http_request.send() => response?,
            };
            let value = xiaomi_voice_json_response(response, &provider_name).await?;
            let transcript = parse_xiaomi_asr_response(&value)?;
            Ok(VoiceAsrResult {
                transcript,
                provider: request.provider,
                model: request.model,
                language: request.language,
                metadata: json!({"provider": provider_name}),
            })
        })
    }
}

impl VoiceTtsProvider for XiaomiVoiceProvider {
    fn synthesize(
        &self,
        request: VoiceTtsRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<VoiceTtsResult>> {
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let api_key = self.api_key.clone();
        let provider_name = self.provider_name.clone();
        Box::pin(async move {
            let mut abort = abort;
            validate_voice_tts_request(&request)?;
            let endpoint = openai_chat_completions_endpoint(&base_url);
            let mut http_request = client
                .post(endpoint)
                .header("accept", "application/json")
                .json(&xiaomi_tts_request_body(&request, false));
            if !api_key.trim().is_empty() {
                http_request = http_request.bearer_auth(api_key);
            }
            let response = tokio::select! {
                biased;
                _ = abort.wait_for_abort() => {
                    return Err(Error::Provider("TTS request aborted".to_string()));
                }
                response = http_request.send() => response?,
            };
            let value = xiaomi_voice_json_response(response, &provider_name).await?;
            let audio = parse_xiaomi_tts_response(&value, request.format)?;
            Ok(VoiceTtsResult {
                audio,
                provider: request.provider,
                model: request.model,
                voice: request.voice,
                metadata: json!({"provider": provider_name}),
            })
        })
    }
}

pub fn validate_voice_asr_request(request: &VoiceAsrRequest) -> Result<()> {
    if !request.audio.format.supports_asr_input() {
        return Err(Error::Provider(format!(
            "unsupported ASR audio format: {}",
            request.audio.format.as_str()
        )));
    }
    if request.audio.data.len() > MAX_VOICE_AUDIO_BASE64_BYTES {
        return Err(Error::Provider(format!(
            "ASR audio exceeds {} encoded bytes",
            MAX_VOICE_AUDIO_BASE64_BYTES
        )));
    }
    Ok(())
}

pub fn validate_voice_tts_request(request: &VoiceTtsRequest) -> Result<()> {
    if !request.format.supports_tts_output() {
        return Err(Error::Provider(format!(
            "unsupported TTS audio format: {}",
            request.format.as_str()
        )));
    }
    if request.text.trim().is_empty() {
        return Err(Error::Provider("TTS text is empty".to_string()));
    }
    Ok(())
}

pub fn xiaomi_asr_request_body(request: &VoiceAsrRequest) -> Value {
    let mut body = json!({
        "model": request.model,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "input_audio",
                "input_audio": {
                    "data": request.audio.data,
                    "format": request.audio.format.as_str(),
                }
            }]
        }],
        "stream": false,
    });
    if let Some(language) = request
        .language
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        body["asr_options"] = json!({ "language": language });
    }
    body
}

pub fn xiaomi_tts_request_body(request: &VoiceTtsRequest, stream: bool) -> Value {
    json!({
        "model": request.model,
        "messages": [{
            "role": "assistant",
            "content": request.text,
        }],
        "audio": {
            "format": request.format.as_str(),
            "voice": request.voice,
        },
        "modalities": ["audio"],
        "stream": stream,
    })
}

pub fn parse_xiaomi_asr_response(value: &Value) -> Result<String> {
    let message = first_choice_message(value).unwrap_or(value);
    for candidate in [
        message.get("transcript"),
        message.get("text"),
        message.get("content"),
        value.get("transcript"),
        value.get("text"),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(text) = candidate
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Ok(text.to_string());
        }
        if let Some(text) = text_from_content_array(candidate) {
            return Ok(text);
        }
    }
    Err(Error::Provider(
        "ASR response did not include transcript text".to_string(),
    ))
}

pub fn parse_xiaomi_tts_response(
    value: &Value,
    format: VoiceAudioFormat,
) -> Result<VoiceAudioOutput> {
    let message = first_choice_message(value).unwrap_or(value);
    let audio = message
        .get("audio")
        .or_else(|| value.get("audio"))
        .unwrap_or(message);
    let data = audio
        .get("data")
        .or_else(|| audio.get("audio"))
        .or_else(|| message.get("data"))
        .or_else(|| value.get("data"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Provider("TTS response did not include audio data".to_string()))?;
    Ok(VoiceAudioOutput {
        data: data.to_string(),
        format,
        mime_type: format.mime_type().to_string(),
    })
}

pub fn parse_xiaomi_tts_sse_audio_delta(value: &Value) -> Option<VoiceAudioChunk> {
    let delta = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))
        .unwrap_or(value);
    let audio = delta.get("audio").unwrap_or(delta);
    let data = audio
        .get("data")
        .or_else(|| audio.get("delta"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|data| !data.is_empty())?;
    let format = audio
        .get("format")
        .and_then(Value::as_str)
        .and_then(parse_voice_audio_format)
        .unwrap_or(VoiceAudioFormat::Pcm16);
    let sample_rate = audio
        .get("sample_rate")
        .or_else(|| audio.get("sampleRate"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    Some(VoiceAudioChunk {
        data: data.to_string(),
        format,
        sample_rate,
    })
}

pub fn parse_voice_audio_format(value: &str) -> Option<VoiceAudioFormat> {
    match value.trim().to_ascii_lowercase().as_str() {
        "wav" | "wave" => Some(VoiceAudioFormat::Wav),
        "mp3" | "mpeg" => Some(VoiceAudioFormat::Mp3),
        "pcm16" | "pcm_s16le" | "s16le" => Some(VoiceAudioFormat::Pcm16),
        _ => None,
    }
}

async fn xiaomi_voice_json_response(
    response: reqwest::Response,
    provider_name: &str,
) -> Result<Value> {
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|err| format!("<failed to read error body: {err}>"));
        return Err(Error::Provider(format!(
            "{provider_name} returned HTTP {status}: {}",
            truncate_provider_body(&body)
        )));
    }
    Ok(response.json::<Value>().await?)
}

fn first_choice_message(value: &Value) -> Option<&Value> {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message").or_else(|| choice.get("delta")))
}

fn text_from_content_array(value: &Value) -> Option<String> {
    let values = value.as_array()?;
    let text = values
        .iter()
        .filter_map(|entry| {
            entry
                .get("text")
                .or_else(|| entry.get("transcript"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("");
    (!text.is_empty()).then_some(text)
}

fn truncate_provider_body(value: &str) -> String {
    let trimmed = value.trim().replace(['\r', '\n', '\t'], " ");
    if trimmed.chars().count() <= 160 {
        trimmed
    } else {
        let mut out = trimmed.chars().take(157).collect::<String>();
        out.push_str("...");
        out
    }
}
