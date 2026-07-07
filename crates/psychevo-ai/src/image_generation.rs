#[allow(unused_imports)]
pub(crate) use super::*;

pub const MAX_IMAGE_GENERATION_INPUTS: usize = 5;
pub const DEFAULT_FAKE_IMAGE_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mOsvmfPfwAH5QMm7n0ViwAAAABJRU5ErkJggg==";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageGenerationFormat {
    Png,
    Jpeg,
    Webp,
}

impl ImageGenerationFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenerationInputImage {
    pub source: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenerationRequest {
    pub provider: String,
    pub model: String,
    pub prompt: String,
    pub aspect_ratio: Option<String>,
    pub image: Option<ImageGenerationInputImage>,
    pub reference_images: Vec<ImageGenerationInputImage>,
    pub size: Option<String>,
    pub format: ImageGenerationFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenerationResult {
    pub data_base64: String,
    pub mime_type: String,
    pub provider: String,
    pub model: String,
    pub revised_prompt: Option<String>,
    pub metadata: Value,
}

pub trait ImageGenerationProvider: Send + Sync {
    fn generate(
        &self,
        request: ImageGenerationRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<ImageGenerationResult>>;
}

#[derive(Debug, Clone)]
pub struct FakeImageGenerationProvider {
    data_base64: String,
    mime_type: String,
}

impl Default for FakeImageGenerationProvider {
    fn default() -> Self {
        Self::new(DEFAULT_FAKE_IMAGE_BASE64, "image/png")
    }
}

impl FakeImageGenerationProvider {
    pub fn new(data_base64: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            data_base64: data_base64.into(),
            mime_type: mime_type.into(),
        }
    }
}

impl ImageGenerationProvider for FakeImageGenerationProvider {
    fn generate(
        &self,
        request: ImageGenerationRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<ImageGenerationResult>> {
        let data_base64 = self.data_base64.clone();
        let mime_type = self.mime_type.clone();
        Box::pin(async move {
            if abort.aborted() {
                return Err(Error::Provider(
                    "image generation request aborted".to_string(),
                ));
            }
            Ok(ImageGenerationResult {
                data_base64,
                mime_type,
                provider: request.provider,
                model: request.model,
                revised_prompt: Some(format!("fake: {}", request.prompt)),
                metadata: json!({
                    "provider": "fake",
                    "input_images": request.image.iter().count() + request.reference_images.len(),
                    "aspect_ratio": request.aspect_ratio,
                    "size": request.size,
                    "format": request.format.as_str(),
                }),
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiImageGenerationProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    provider_name: String,
}

impl OpenAiImageGenerationProvider {
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

impl ImageGenerationProvider for OpenAiImageGenerationProvider {
    fn generate(
        &self,
        request: ImageGenerationRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<ImageGenerationResult>> {
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let api_key = self.api_key.clone();
        let provider_name = self.provider_name.clone();
        Box::pin(async move {
            let mut abort = abort;
            validate_image_generation_request(&request)?;
            if request.image.is_some() || !request.reference_images.is_empty() {
                return Err(Error::Provider(
                    "OpenAI image edits with input images are not enabled in this runtime build"
                        .to_string(),
                ));
            }
            let endpoint = openai_image_generations_endpoint(&base_url);
            let mut http_request = client
                .post(endpoint)
                .header("accept", "application/json")
                .json(&openai_image_generation_request_body(&request));
            if !api_key.trim().is_empty() {
                http_request = http_request.bearer_auth(&api_key);
            }
            let response = tokio::select! {
                biased;
                _ = abort.wait_for_abort() => {
                    return Err(Error::Provider("image generation request aborted".to_string()));
                }
                response = http_request.send() => response?,
            };
            let value = openai_image_json_response(response, &provider_name).await?;
            parse_openai_image_generation_response(value, &request, &provider_name)
        })
    }
}

pub fn validate_image_generation_request(request: &ImageGenerationRequest) -> Result<()> {
    if request.prompt.trim().is_empty() {
        return Err(Error::Provider(
            "image generation prompt is empty".to_string(),
        ));
    }
    let input_count = request.image.iter().count() + request.reference_images.len();
    if input_count > MAX_IMAGE_GENERATION_INPUTS {
        return Err(Error::Provider(format!(
            "image generation accepts at most {MAX_IMAGE_GENERATION_INPUTS} input images"
        )));
    }
    Ok(())
}

pub fn openai_image_generations_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/images/generations") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/images/generations")
    } else {
        format!("{trimmed}/v1/images/generations")
    }
}

pub fn openai_image_generation_request_body(request: &ImageGenerationRequest) -> Value {
    let mut body = json!({
        "model": request.model,
        "prompt": request.prompt,
        "n": 1,
        "output_format": request.format.as_str(),
    });
    if let Some(size) = request
        .size
        .as_deref()
        .map(str::trim)
        .filter(|size| !size.is_empty())
    {
        body["size"] = json!(size);
    }
    if let Some(aspect_ratio) = request
        .aspect_ratio
        .as_deref()
        .map(str::trim)
        .filter(|aspect_ratio| !aspect_ratio.is_empty())
    {
        body["aspect_ratio"] = json!(aspect_ratio);
    }
    body
}

pub fn parse_openai_image_generation_response(
    value: Value,
    request: &ImageGenerationRequest,
    provider_name: &str,
) -> Result<ImageGenerationResult> {
    let first = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|data| data.first())
        .ok_or_else(|| {
            Error::Provider(format!(
                "{provider_name} image response did not include generated image data"
            ))
        })?;
    let data_base64 = first
        .get("b64_json")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::Provider(format!(
                "{provider_name} image response omitted b64_json output"
            ))
        })?;
    Ok(ImageGenerationResult {
        data_base64: data_base64.to_string(),
        mime_type: request.format.mime_type().to_string(),
        provider: request.provider.clone(),
        model: request.model.clone(),
        revised_prompt: first
            .get("revised_prompt")
            .and_then(Value::as_str)
            .map(str::to_string),
        metadata: json!({
            "provider": provider_name,
            "created": value.get("created").cloned().unwrap_or(Value::Null),
            "usage": value.get("usage").cloned().unwrap_or(Value::Null),
            "output_format": value.get("output_format").cloned().unwrap_or_else(|| json!(request.format.as_str())),
            "size": value.get("size").cloned().unwrap_or_else(|| json!(request.size)),
            "quality": value.get("quality").cloned().unwrap_or(Value::Null),
        }),
    })
}

async fn openai_image_json_response(
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
            truncate_image_provider_body(&body)
        )));
    }
    Ok(response.json::<Value>().await?)
}

fn truncate_image_provider_body(value: &str) -> String {
    let trimmed = value.trim().replace(['\r', '\n', '\t'], " ");
    if trimmed.chars().count() <= 160 {
        trimmed
    } else {
        let mut out = trimmed.chars().take(157).collect::<String>();
        out.push_str("...");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abort_signal() -> AbortSignal {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        AbortSignal::new(rx)
    }

    #[test]
    fn openai_image_request_body_uses_generation_endpoint_shape() {
        let request = ImageGenerationRequest {
            provider: "openai".to_string(),
            model: "gpt-image-2".to_string(),
            prompt: "a diagram".to_string(),
            aspect_ratio: Some("16:9".to_string()),
            image: None,
            reference_images: Vec::new(),
            size: Some("1536x1024".to_string()),
            format: ImageGenerationFormat::Webp,
        };

        let body = openai_image_generation_request_body(&request);

        assert_eq!(body["model"], "gpt-image-2");
        assert_eq!(body["prompt"], "a diagram");
        assert_eq!(body["n"], 1);
        assert_eq!(body["output_format"], "webp");
        assert_eq!(body["size"], "1536x1024");
        assert_eq!(body["aspect_ratio"], "16:9");
        assert_eq!(
            openai_image_generations_endpoint("https://api.openai.com/v1"),
            "https://api.openai.com/v1/images/generations"
        );
    }

    #[tokio::test]
    async fn openai_image_provider_rejects_input_images_until_edits_are_enabled() {
        let provider =
            OpenAiImageGenerationProvider::new("http://127.0.0.1:9/v1", "test-key", "openai");
        let request = ImageGenerationRequest {
            provider: "openai".to_string(),
            model: "gpt-image-2".to_string(),
            prompt: "edit this".to_string(),
            aspect_ratio: None,
            image: Some(ImageGenerationInputImage {
                source: "psychevo-media://img_test".to_string(),
                mime_type: Some("image/png".to_string()),
            }),
            reference_images: Vec::new(),
            size: Some("1024x1024".to_string()),
            format: ImageGenerationFormat::Png,
        };

        let err = provider
            .generate(request, abort_signal())
            .await
            .expect_err("edit inputs disabled");
        assert!(err.to_string().contains("image edits with input images"));
    }
}
