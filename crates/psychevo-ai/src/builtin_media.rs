use std::collections::BTreeMap;

#[cfg(feature = "openai")]
use futures::StreamExt;
#[cfg(any(feature = "openai", feature = "xiaomi"))]
use serde_json::Value;
#[cfg(feature = "openai")]
use serde_json::json;

use crate::{AdapterCall, AdapterFuture, ErrorPhase, Media, ProviderError};
#[cfg(feature = "openai")]
use crate::{
    ErrorKind, GeneratedImage, ImageAdapter, ImageAdapterOutput, ImageRequest, Usage,
    normalize_usage,
};
#[cfg(feature = "xiaomi")]
use crate::{
    MediaInput, SpeechAdapter, SpeechAdapterOutput, SpeechRequest, TranscriptionAdapter,
    TranscriptionAdapterOutput, TranscriptionRequest, VoiceAsrProvider, VoiceAsrRequest,
    VoiceAudioFormat, VoiceAudioInput, VoiceTtsProvider, VoiceTtsRequest, Warning,
    XiaomiVoiceProvider,
};

#[cfg(feature = "openai")]
const PROVIDER_BODY_LIMIT: usize = 64 * 1024;

#[cfg(feature = "openai")]
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAiImageAdapter;

#[cfg(feature = "openai")]
impl ImageAdapter for OpenAiImageAdapter {
    fn generate(&self, call: AdapterCall<ImageRequest>) -> AdapterFuture<'_, ImageAdapterOutput> {
        Box::pin(async move {
            if call.request.prompt.trim().is_empty() {
                return Err(ProviderError::invalid_request(
                    "image prompt must not be empty",
                ));
            }
            if !call.request.input_images.is_empty() {
                return Err(ProviderError::invalid_request(
                    "OpenAI image generation does not support input images in this Adapter",
                ));
            }
            let format = normalized_image_format(call.request.format.as_deref())?;
            let endpoint = openai_image_generations_endpoint(&call.context.endpoint);
            let mut body = json!({
                "model": call.model,
                "prompt": call.request.prompt,
                "n": call.request.count,
                "output_format": format,
            });
            if let Some(size) = call.request.size {
                body["size"] = Value::String(size);
            }
            if let Some(aspect_ratio) = call.request.aspect_ratio {
                body["aspect_ratio"] = Value::String(aspect_ratio);
            }
            for (namespace, value) in call.request.extensions {
                if namespace != "openai" {
                    continue;
                }
                if let Some(object) = value.as_object() {
                    for (key, value) in object {
                        if !body.get(key).is_some() {
                            body[key] = value.clone();
                        }
                    }
                }
            }
            let mut request = call
                .context
                .client
                .post(endpoint)
                .header("accept", "application/json")
                .json(&body);
            for (name, value) in &call.context.headers {
                request = request.header(name, value);
            }
            if let Some(api_key) = call.context.credentials.get("api_key") {
                request = request.bearer_auth(api_key.expose_secret());
            }
            let response = request.send().await.map_err(|error| {
                ProviderError::new(
                    ErrorKind::Transport,
                    ErrorPhase::Dispatch,
                    format!("OpenAI image request failed: {error}"),
                )
            })?;
            let status = response.status();
            if !status.is_success() {
                let retry_after_seconds = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<u64>().ok());
                let body = bounded_response_body(response).await;
                return Err(ProviderError::provider(
                    ErrorPhase::ResponseBody,
                    Some(status.as_u16()),
                    provider_error_code(&body),
                    provider_error_message(&body),
                )
                .with_retry_after_seconds(retry_after_seconds));
            }
            let value = response.json::<Value>().await.map_err(|error| {
                ProviderError::new(
                    ErrorKind::Protocol,
                    ErrorPhase::ResponseBody,
                    format!("OpenAI image response JSON failed: {error}"),
                )
            })?;
            parse_openai_images(value, format)
        })
    }
}

#[cfg(feature = "openai")]
fn parse_openai_images(value: Value, format: &str) -> Result<ImageAdapterOutput, ProviderError> {
    let values = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::protocol("image response did not contain a data array"))?;
    let mime_type = match format {
        "jpeg" | "jpg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    };
    let mut images = Vec::with_capacity(values.len());
    for item in values {
        let data = item
            .get("b64_json")
            .or_else(|| item.get("data"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::protocol("image response item did not contain inline base64 media")
            })?;
        images.push(GeneratedImage {
            media: Media::from_base64(mime_type, data),
            revised_prompt: item
                .get("revised_prompt")
                .and_then(Value::as_str)
                .map(str::to_string),
            provider_metadata: item
                .get("metadata")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect(),
        });
    }
    Ok(ImageAdapterOutput {
        images,
        usage: value.get("usage").and_then(typed_usage),
        warnings: Vec::new(),
        provider_metadata: allowlisted_metadata(&value),
    })
}

#[cfg(feature = "openai")]
fn openai_image_generations_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/images/generations") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/images/generations")
    } else {
        format!("{trimmed}/v1/images/generations")
    }
}

#[cfg(feature = "xiaomi")]
#[derive(Debug, Clone, Copy, Default)]
pub struct XiaomiTranscriptionAdapter;

#[cfg(feature = "xiaomi")]
impl TranscriptionAdapter for XiaomiTranscriptionAdapter {
    fn transcribe(
        &self,
        call: AdapterCall<TranscriptionRequest>,
    ) -> AdapterFuture<'_, TranscriptionAdapterOutput> {
        Box::pin(async move {
            let (audio_data, format, mime_type) = inline_audio(&call.request.audio, true)?;
            let api_key = call
                .context
                .credentials
                .get("api_key")
                .map(|secret| secret.expose_secret().to_string())
                .unwrap_or_default();
            let provider = XiaomiVoiceProvider {
                client: call.context.client,
                base_url: call.context.endpoint,
                api_key,
                provider_name: call.context.model.provider_family.clone(),
                headers: call.context.headers,
            };
            let result = provider
                .transcribe(
                    VoiceAsrRequest {
                        provider: call.context.model.provider_family,
                        model: call.model,
                        language: call.request.language.clone(),
                        audio: VoiceAudioInput {
                            data: audio_data,
                            format,
                            mime_type: Some(mime_type),
                        },
                    },
                    call.context.abort,
                )
                .await
                .map_err(|error| crate::sdk_error::legacy_error(error, ErrorPhase::ResponseBody))?;
            let mut warnings = Vec::new();
            if call.request.prompt.is_some() {
                warnings.push(Warning::new(
                    "unsupported_transcription_prompt",
                    "Xiaomi transcription omitted the prompt preference",
                ));
            }
            Ok(TranscriptionAdapterOutput {
                text: result.transcript,
                segments: Vec::new(),
                language: result.language,
                duration_seconds: None,
                warnings,
                provider_metadata: value_object(result.metadata),
            })
        })
    }
}

#[cfg(feature = "xiaomi")]
#[derive(Debug, Clone, Copy, Default)]
pub struct XiaomiSpeechAdapter;

#[cfg(feature = "xiaomi")]
impl SpeechAdapter for XiaomiSpeechAdapter {
    fn synthesize(
        &self,
        call: AdapterCall<SpeechRequest>,
    ) -> AdapterFuture<'_, SpeechAdapterOutput> {
        Box::pin(async move {
            let format = speech_format(call.request.format.as_deref())?;
            let api_key = call
                .context
                .credentials
                .get("api_key")
                .map(|secret| secret.expose_secret().to_string())
                .unwrap_or_default();
            let provider = XiaomiVoiceProvider {
                client: call.context.client,
                base_url: call.context.endpoint,
                api_key,
                provider_name: call.context.model.provider_family.clone(),
                headers: call.context.headers,
            };
            let result = provider
                .synthesize(
                    VoiceTtsRequest {
                        provider: call.context.model.provider_family,
                        model: call.model,
                        voice: call.request.voice.unwrap_or_else(|| "default".to_string()),
                        format,
                        text: call.request.text,
                    },
                    call.context.abort,
                )
                .await
                .map_err(|error| crate::sdk_error::legacy_error(error, ErrorPhase::ResponseBody))?;
            let mut warnings = Vec::new();
            if call.request.speed.is_some() {
                warnings.push(Warning::new(
                    "unsupported_speech_speed",
                    "Xiaomi speech omitted the speed preference",
                ));
            }
            Ok(SpeechAdapterOutput {
                audio: Media::from_base64(result.audio.mime_type, result.audio.data),
                warnings,
                provider_metadata: value_object(result.metadata),
            })
        })
    }
}

#[cfg(feature = "xiaomi")]
fn inline_audio(
    input: &MediaInput,
    asr: bool,
) -> Result<(String, VoiceAudioFormat, String), ProviderError> {
    let MediaInput::Inline { media } = input else {
        return Err(ProviderError::invalid_request(
            "Xiaomi voice requires inline audio media",
        ));
    };
    let format = audio_format(media.mime_type()).ok_or_else(|| {
        ProviderError::invalid_request(format!(
            "unsupported {} audio MIME type `{}`",
            if asr { "transcription" } else { "speech" },
            media.mime_type()
        ))
    })?;
    if asr && !format.supports_asr_input() {
        return Err(ProviderError::invalid_request(format!(
            "unsupported transcription audio format `{}`",
            format.as_str()
        )));
    }
    Ok((
        media
            .base64()
            .map_err(|error| ProviderError::invalid_request(error.to_string()))?
            .to_string(),
        format,
        media.mime_type().to_string(),
    ))
}

#[cfg(feature = "xiaomi")]
fn audio_format(mime_type: &str) -> Option<VoiceAudioFormat> {
    match mime_type.to_ascii_lowercase().as_str() {
        "audio/wav" | "audio/wave" | "audio/x-wav" => Some(VoiceAudioFormat::Wav),
        "audio/mpeg" | "audio/mp3" => Some(VoiceAudioFormat::Mp3),
        "audio/pcm" | "audio/l16" => Some(VoiceAudioFormat::Pcm16),
        _ => None,
    }
}

#[cfg(feature = "xiaomi")]
fn speech_format(value: Option<&str>) -> Result<VoiceAudioFormat, ProviderError> {
    match value.unwrap_or("wav").to_ascii_lowercase().as_str() {
        "wav" | "wave" => Ok(VoiceAudioFormat::Wav),
        "pcm16" | "pcm" => Ok(VoiceAudioFormat::Pcm16),
        value => Err(ProviderError::invalid_request(format!(
            "unsupported Xiaomi speech format `{value}`"
        ))),
    }
}

#[cfg(feature = "openai")]
fn normalized_image_format(value: Option<&str>) -> Result<&str, ProviderError> {
    match value.unwrap_or("png").to_ascii_lowercase().as_str() {
        "png" => Ok("png"),
        "jpeg" | "jpg" => Ok("jpeg"),
        "webp" => Ok("webp"),
        value => Err(ProviderError::invalid_request(format!(
            "unsupported image format `{value}`"
        ))),
    }
}

#[cfg(feature = "openai")]
async fn bounded_response_body(response: reqwest::Response) -> Value {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(next) = stream.next().await {
        let Ok(next) = next else {
            break;
        };
        let remaining = PROVIDER_BODY_LIMIT.saturating_sub(bytes.len());
        bytes.extend_from_slice(&next[..next.len().min(remaining)]);
        if bytes.len() >= PROVIDER_BODY_LIMIT {
            break;
        }
    }
    serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        json!({
            "error": {
                "message": String::from_utf8_lossy(&bytes),
            }
        })
    })
}

#[cfg(feature = "openai")]
fn provider_error_code(value: &Value) -> Option<String> {
    value
        .pointer("/error/code")
        .or_else(|| value.get("code"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(feature = "openai")]
fn provider_error_message(value: &Value) -> String {
    value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("provider returned an unsuccessful response")
        .to_string()
}

#[cfg(feature = "openai")]
fn typed_usage(value: &Value) -> Option<Usage> {
    let normalized = normalize_usage(value)?;
    Some(Usage {
        input_tokens: normalized.get("input_tokens").and_then(Value::as_u64),
        output_tokens: normalized.get("output_tokens").and_then(Value::as_u64),
        total_tokens: normalized.get("total_tokens").and_then(Value::as_u64),
        reasoning_tokens: normalized.get("reasoning_tokens").and_then(Value::as_u64),
        cached_tokens: normalized.get("cached_tokens").and_then(Value::as_u64),
        cache_write_tokens: normalized.get("cache_write_tokens").and_then(Value::as_u64),
        provider_reported_cost: None,
        provider_metadata: BTreeMap::new(),
    })
}

#[cfg(feature = "openai")]
fn allowlisted_metadata(value: &Value) -> BTreeMap<String, Value> {
    crate::allowlisted_provider_metadata(value)
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

#[cfg(feature = "xiaomi")]
fn value_object(value: Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "openai")]
    #[test]
    fn openai_image_response_is_typed_and_preserves_safe_metadata() {
        let output = parse_openai_images(
            json!({
                "data": [{
                    "b64_json": "cG5n",
                    "revised_prompt": "revised",
                    "metadata": {"seed": 7}
                }],
                "usage": {"total_tokens": 12},
                "id": "image-response"
            }),
            "png",
        )
        .expect("image output");

        assert_eq!(output.images.len(), 1);
        assert_eq!(output.images[0].media.mime_type(), "image/png");
        assert_eq!(output.images[0].media.base64().expect("base64"), "cG5n");
        assert_eq!(output.images[0].revised_prompt.as_deref(), Some("revised"));
        assert_eq!(output.images[0].provider_metadata["seed"], 7);
        assert_eq!(
            output.usage.as_ref().and_then(|usage| usage.total_tokens),
            Some(12)
        );
    }

    #[cfg(feature = "openai")]
    #[test]
    fn openai_image_response_requires_inline_media() {
        let error =
            parse_openai_images(json!({"data": [{"url": "https://example.test/x"}]}), "png")
                .expect_err("URL-only result");
        assert_eq!(error.kind, ErrorKind::Protocol);
    }

    #[cfg(feature = "openai")]
    #[test]
    fn openai_image_endpoint_and_format_are_normalized() {
        assert_eq!(
            openai_image_generations_endpoint("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/images/generations"
        );
        assert_eq!(
            openai_image_generations_endpoint("https://proxy.example"),
            "https://proxy.example/v1/images/generations"
        );
        assert_eq!(
            openai_image_generations_endpoint("https://api.openai.com/v1/images/generations"),
            "https://api.openai.com/v1/images/generations"
        );
        assert_eq!(normalized_image_format(Some("JPG")).expect("jpeg"), "jpeg");
        assert!(normalized_image_format(Some("gif")).is_err());
    }

    #[cfg(feature = "xiaomi")]
    #[test]
    fn xiaomi_voice_formats_are_explicit() {
        assert_eq!(audio_format("audio/wav"), Some(VoiceAudioFormat::Wav));
        assert_eq!(audio_format("audio/mpeg"), Some(VoiceAudioFormat::Mp3));
        assert_eq!(
            audio_format("application/octet-stream"),
            None,
            "unknown media must not be guessed"
        );
        assert_eq!(
            speech_format(Some("pcm16")).expect("pcm"),
            VoiceAudioFormat::Pcm16
        );
        assert!(speech_format(Some("mp3")).is_err());
    }
}
