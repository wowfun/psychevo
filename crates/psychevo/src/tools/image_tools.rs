use psychevo_ai::{
    DeploymentConfig, Fake, ImageModel, ImageRequest, MediaInput, OpenAi, SecretValue,
};

use super::*;

pub(crate) const MAX_IMAGE_TOOL_INPUTS: usize = 5;

pub(crate) struct ViewImageTool {
    cwd: PathBuf,
    context: ToolRuntimeContext,
}

impl ViewImageTool {
    pub(crate) fn new(cwd: PathBuf, context: ToolRuntimeContext) -> Self {
        Self { cwd, context }
    }
}

impl ToolBinding for ViewImageTool {
    fn name(&self) -> &str {
        "view_image"
    }

    fn description(&self) -> &str {
        "Inspect an image."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Local path, file URL, data image URL, remote image URL, or media reference."
                },
                "detail": {
                    "type": "string",
                    "enum": ["low", "high", "original"],
                    "description": "Inspection detail."
                }
            },
            "required": ["source"],
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let cwd = self.cwd.clone();
        let context = self.context.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("view_image request aborted");
            }
            let Some(source) = args
                .get("source")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|source| !source.is_empty())
            else {
                return ToolOutput::error("source is required");
            };
            let home = context
                .home
                .clone()
                .unwrap_or_else(|| cwd.join(".psychevo"));
            match crate::media::resolve_explicit_image_source(source, &cwd, &home).await {
                Ok(resolved) => {
                    let json = json!({
                        "source": source,
                        "displaySource": resolved.display_source,
                        "agentVisibleSource": resolved.agent_visible_source,
                        "mimeType": resolved.mime_type,
                        "width": resolved.width,
                        "height": resolved.height,
                        "sizeBytes": resolved.size_bytes,
                        "detail": args.get("detail").and_then(Value::as_str).unwrap_or("high"),
                        "imageInput": if context.image_input_enabled { "attached" } else { "text_only" },
                        "content": view_image_model_text(&resolved, context.image_input_enabled),
                    });
                    let output = ToolOutput::ok_with_model_content(
                        json,
                        view_image_model_text(&resolved, context.image_input_enabled),
                    );
                    if context.image_input_enabled {
                        output.with_attachment(ToolAttachment::ImageUrl {
                            url: resolved.data_url,
                            mime_type: resolved.mime_type,
                            source_url: Some(resolved.display_source),
                        })
                    } else {
                        output
                    }
                }
                Err(err) => ToolOutput::error(err.to_string()),
            }
        })
    }
}

pub(crate) struct ImageGenerateTool {
    cwd: PathBuf,
    context: ToolRuntimeContext,
}

impl ImageGenerateTool {
    pub(crate) fn new(cwd: PathBuf, context: ToolRuntimeContext) -> Self {
        Self { cwd, context }
    }
}

impl ToolBinding for ImageGenerateTool {
    fn name(&self) -> &str {
        "image_generate"
    }

    fn canonical_tool_name(&self) -> psychevo_ai::ToolName {
        psychevo_ai::ToolName::namespaced("image_generation", "generate")
    }

    fn description(&self) -> &str {
        "Generate an image from a prompt."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Text description of the image to generate."
                },
                "aspect_ratio": {
                    "type": "string",
                    "description": "Optional output aspect ratio hint such as 1:1, 4:3, or 16:9."
                },
                "image_url": {
                    "type": "string",
                    "description": "Optional explicit source image URL/path/reference."
                },
                "reference_image_urls": {
                    "type": "array",
                    "items": { "type": "string" },
                    "maxItems": 5,
                    "description": "Optional explicit reference image URL/path/reference list."
                },
                "num_recent_images": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 5,
                    "description": "Optional count of recent generated images to reuse when available."
                }
            },
            "required": ["prompt"],
            "additionalProperties": false
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let cwd = self.cwd.clone();
        let context = self.context.clone();
        Box::pin(async move {
            let mut abort = abort;
            let Some(prompt) = args
                .get("prompt")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|prompt| !prompt.is_empty())
            else {
                return ToolOutput::error("prompt is required");
            };
            let Some(config) = context.image_generation.clone() else {
                return ToolOutput::error("image generation is not configured");
            };
            let home = context
                .home
                .clone()
                .unwrap_or_else(|| cwd.join(".psychevo"));
            let aspect_ratio = args
                .get("aspect_ratio")
                .or_else(|| args.get("aspectRatio"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let explicit_image = args
                .get("image_url")
                .or_else(|| args.get("imageUrl"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let reference_images = args
                .get("reference_image_urls")
                .or_else(|| args.get("referenceImageUrls"))
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let requested_recent = args
                .get("num_recent_images")
                .or_else(|| args.get("numRecentImages"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(MAX_IMAGE_TOOL_INPUTS as u64) as usize;
            let explicit_count = explicit_image.iter().count() + reference_images.len();
            if explicit_count + requested_recent > MAX_IMAGE_TOOL_INPUTS {
                return ToolOutput::error(format!(
                    "image generation accepts at most {MAX_IMAGE_TOOL_INPUTS} total input images"
                ));
            }
            let input_image = match explicit_image {
                Some(source) => match resolve_generation_input_image(source, &cwd, &home).await {
                    Ok(image) => Some(image),
                    Err(err) => return ToolOutput::error(err.to_string()),
                },
                None => None,
            };
            let mut resolved_references = Vec::new();
            for source in reference_images {
                match resolve_generation_input_image(&source, &cwd, &home).await {
                    Ok(image) => resolved_references.push(image),
                    Err(err) => return ToolOutput::error(err.to_string()),
                }
            }
            let request = ImageRequest {
                prompt: prompt.to_string(),
                count: 1,
                aspect_ratio,
                input_images: input_image
                    .into_iter()
                    .chain(resolved_references)
                    .collect(),
                size: Some(config.size.clone()),
                format: Some(config.format.as_str().to_string()),
                headers: BTreeMap::new(),
                extensions: BTreeMap::new(),
            };
            let provider = match image_generation_provider(&config) {
                Ok(provider) => provider,
                Err(err) => return ToolOutput::error(err.to_string()),
            };
            let mut invocation = provider.generate(request);
            let result = tokio::select! {
                result = &mut invocation => result,
                _ = abort.wait_for_abort() => {
                    invocation.abort();
                    invocation.await
                }
            };
            let result = match result {
                Ok(result) => result,
                Err(err) => return ToolOutput::error(err.to_string()),
            };
            let Some(image) = result.images.first() else {
                return ToolOutput::error("image provider returned no images");
            };
            let bytes = match image.media.bytes() {
                Ok(bytes) => bytes,
                Err(err) => {
                    return ToolOutput::error(format!(
                        "image provider returned invalid media: {err}"
                    ));
                }
            };
            let artifact = match crate::media::write_generated_image_artifact(
                &home,
                bytes.as_ref(),
                image.media.mime_type(),
            ) {
                Ok(artifact) => artifact,
                Err(err) => return ToolOutput::error(err.to_string()),
            };
            let output = json!({
                "display": "Generated image",
                "status": "completed",
                "mediaKind": "generated_image",
                "artifactId": artifact.artifact_id,
                "mimeType": artifact.mime_type,
                "prompt": prompt,
                "provider": result.model.provider_family,
                "model": result.model.model_id,
                "savedPath": artifact.saved_path.display().to_string(),
                "displayUrl": artifact.display_url,
                "agentVisibleSource": artifact.agent_visible_source,
                "revisedPrompt": image.revised_prompt,
                "width": artifact.width,
                "height": artifact.height,
                "sizeBytes": artifact.size_bytes,
                "recentImagesRequested": requested_recent,
                "recentImagesSelected": 0,
                "providerMetadata": result.provider_metadata,
            });
            let model_content = serde_json::to_string(&output)
                .unwrap_or_else(|_| "{\"status\":\"completed\"}".to_string());
            ToolOutput::ok_with_model_content(output, model_content)
        })
    }
}

async fn resolve_generation_input_image(
    source: &str,
    cwd: &Path,
    home: &Path,
) -> Result<MediaInput> {
    let resolved = crate::media::resolve_explicit_image_source(source, cwd, home).await?;
    Ok(MediaInput::Url {
        url: resolved.agent_visible_source,
        mime_type: Some(resolved.mime_type),
    })
}

fn image_generation_provider(
    config: &ResolvedImageGenerationConfig,
) -> Result<ImageModel> {
    match config.provider.as_str() {
        "fake" => Fake::new()
            .map_err(|error| Error::Config(error.to_string()))?
            .provider()
            .image_model(config.model.clone())
            .map_err(|error| Error::Config(error.to_string())),
        provider => {
            let api_key = config.api_key.clone().ok_or_else(|| {
                Error::Config(format!(
                    "{} is required for {provider} image generation",
                    config.api_key_env.as_deref().unwrap_or("provider API key")
                ))
            })?;
            OpenAi::builder(
                DeploymentConfig::new(
                    config.provider.clone(),
                    config.provider.clone(),
                    config.base_url.clone(),
                )
                .with_default_language_protocol("openai_chat"),
            )
            .with_api_key(SecretValue::new(api_key))
            .build()
            .and_then(|facade| facade.image(config.model.clone()))
            .map_err(|error| Error::Config(error.to_string()))
        }
    }
}

fn view_image_model_text(
    resolved: &crate::media::ResolvedImageSource,
    image_input_enabled: bool,
) -> String {
    let dimensions = match (resolved.width, resolved.height) {
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "unknown dimensions".to_string(),
    };
    if image_input_enabled {
        format!(
            "Image resolved: {} ({}, {}, {} bytes). The image is attached for model inspection.",
            resolved.display_source, resolved.mime_type, dimensions, resolved.size_bytes
        )
    } else {
        format!(
            "Image resolved but the active model is text-only: {} ({}, {}, {} bytes).",
            resolved.display_source, resolved.mime_type, dimensions, resolved.size_bytes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};

    fn abort_signal() -> AbortSignal {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        AbortSignal::new(rx)
    }

    fn write_png(cwd: &Path) {
        fs::create_dir_all(cwd).expect("cwd");
        let bytes = BASE64_STANDARD
            .decode(psychevo_ai::DEFAULT_FAKE_IMAGE_BASE64)
            .expect("png fixture");
        fs::write(cwd.join("pixel.png"), bytes).expect("image");
    }

    #[tokio::test]
    async fn view_image_attaches_pixels_when_image_input_is_enabled() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        let home = temp.path().join(".psychevo");
        write_png(&cwd);
        let tool = ViewImageTool::new(
            cwd.clone(),
            ToolRuntimeContext {
                home: Some(home),
                image_input_enabled: true,
                ..ToolRuntimeContext::default()
            },
        );

        let output = tool
            .execute(
                "call-view".to_string(),
                json!({ "source": "pixel.png" }),
                abort_signal(),
            )
            .await;

        assert!(!output.is_error, "{:?}", output.json);
        assert_eq!(output.json["imageInput"], "attached");
        assert_eq!(output.json["mimeType"], "image/png");
        assert_eq!(output.attachments.len(), 1);
        assert!(matches!(
            &output.attachments[0],
            ToolAttachment::ImageUrl { mime_type, .. } if mime_type == "image/png"
        ));
    }

    #[tokio::test]
    async fn view_image_degrades_to_text_for_text_only_models() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        let home = temp.path().join(".psychevo");
        write_png(&cwd);
        let tool = ViewImageTool::new(
            cwd.clone(),
            ToolRuntimeContext {
                home: Some(home),
                image_input_enabled: false,
                ..ToolRuntimeContext::default()
            },
        );

        let output = tool
            .execute(
                "call-view".to_string(),
                json!({ "source": "pixel.png" }),
                abort_signal(),
            )
            .await;

        assert!(!output.is_error, "{:?}", output.json);
        assert_eq!(output.json["imageInput"], "text_only");
        assert!(output.attachments.is_empty());
        assert!(output.model_content().contains("active model is text-only"));
    }

    #[tokio::test]
    async fn image_generate_fake_provider_persists_media_artifact() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        let home = temp.path().join(".psychevo");
        fs::create_dir_all(&cwd).expect("cwd");
        let tool = ImageGenerateTool::new(
            cwd,
            ToolRuntimeContext {
                home: Some(home.clone()),
                image_generation: Some(ResolvedImageGenerationConfig {
                    provider: "fake".to_string(),
                    display_label: "Fake".to_string(),
                    model: "fake-image".to_string(),
                    base_url: "fake://image-generation".to_string(),
                    api_key_env: None,
                    api_key: None,
                    size: "1024x1024".to_string(),
                    format: crate::config::ImageGenerationFormat::Png,
                }),
                ..ToolRuntimeContext::default()
            },
        );

        let output = tool
            .execute(
                "call-image".to_string(),
                json!({ "prompt": "a tiny square", "aspect_ratio": "1:1" }),
                abort_signal(),
            )
            .await;

        assert!(!output.is_error, "{:?}", output.json);
        assert_eq!(output.json["mediaKind"], "generated_image");
        assert_eq!(output.json["provider"], "fake");
        assert_eq!(output.json["model"], "fake-image");
        assert!(
            output.json["displayUrl"]
                .as_str()
                .is_some_and(|value| value.starts_with("/_gateway/media/img_"))
        );
        assert!(
            output.json["agentVisibleSource"]
                .as_str()
                .is_some_and(|value| value.starts_with("psychevo-media://img_"))
        );
        let saved_path = output.json["savedPath"].as_str().expect("saved path");
        assert!(std::path::Path::new(saved_path).is_file());
        let artifact_id = output.json["artifactId"].as_str().expect("artifact id");
        let reread =
            crate::media::read_media_artifact(&home, artifact_id).expect("read generated artifact");
        assert_eq!(reread.mime_type, "image/png");
    }
}
