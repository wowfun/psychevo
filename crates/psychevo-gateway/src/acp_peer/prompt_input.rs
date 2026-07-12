use super::*;

pub(super) async fn acp_prompt_blocks(
    peer: &ResolvedPeerTurn,
    turn: &AcpPeerTurnContext,
    capabilities: &AgentCapabilities,
) -> psychevo_runtime::Result<Vec<ContentBlock>> {
    if !turn.images.is_empty() && !capabilities.prompt_capabilities.image {
        return Err(Error::Message(format!(
            "ACP peer `{}` does not advertise image prompt capability",
            peer.backend.id
        )));
    }

    let mut prompt = Vec::new();
    if let Some(instructions) = turn
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|instructions| !instructions.is_empty())
    {
        if capabilities.prompt_capabilities.embedded_context {
            let resource = TextResourceContents::new(
                instructions,
                format!("psychevo://agent-instructions/{}", peer.agent.name),
            )
            .mime_type("text/markdown".to_string());
            prompt.push(ContentBlock::Resource(EmbeddedResource::new(
                EmbeddedResourceResource::TextResourceContents(resource),
            )));
        } else {
            prompt.push(ContentBlock::Text(TextContent::new(instructions)));
        }
    }
    if turn.input.is_empty() {
        if !turn.prompt.is_empty() {
            prompt.push(ContentBlock::Text(TextContent::new(turn.prompt.clone())));
        }
        for image in &turn.images {
            prompt.push(acp_image_block(image, turn).await?);
        }
    } else {
        for part in &turn.input {
            match part {
                wire::GatewayInputPart::Text { text } if !text.is_empty() => {
                    prompt.push(ContentBlock::Text(TextContent::new(text.clone())));
                }
                wire::GatewayInputPart::Text { .. } => {}
                wire::GatewayInputPart::Image { input } => {
                    let image = match input {
                        wire::GatewayImageInput::LocalPath { path } => {
                            ImageInput::LocalPath(path.into())
                        }
                        wire::GatewayImageInput::Url { url } => ImageInput::ImageUrl(url.clone()),
                    };
                    prompt.push(acp_image_block(&image, turn).await?);
                }
                wire::GatewayInputPart::Context {
                    label,
                    text,
                    visible_to_model: true,
                } => {
                    if !capabilities.prompt_capabilities.embedded_context {
                        return Err(Error::Message(format!(
                            "ACP peer `{}` does not advertise embedded-context prompt capability",
                            peer.backend.id
                        )));
                    }
                    let resource = TextResourceContents::new(
                        text,
                        format!("psychevo://context/{}", url_component(label)),
                    )
                    .mime_type("text/plain".to_string());
                    prompt.push(ContentBlock::Resource(EmbeddedResource::new(
                        EmbeddedResourceResource::TextResourceContents(resource),
                    )));
                }
                wire::GatewayInputPart::Context { .. } => {}
                wire::GatewayInputPart::Resource {
                    uri,
                    mime_type,
                    text,
                    blob,
                } => {
                    if !capabilities.prompt_capabilities.embedded_context {
                        return Err(Error::Message(format!(
                            "ACP peer `{}` does not advertise embedded-context prompt capability",
                            peer.backend.id
                        )));
                    }
                    let resource = match (text, blob) {
                        (Some(text), None) => EmbeddedResourceResource::TextResourceContents(
                            TextResourceContents::new(text, uri).mime_type(mime_type.clone()),
                        ),
                        (None, Some(blob)) => {
                            BASE64_STANDARD.decode(blob).map_err(|error| {
                                Error::Message(format!(
                                    "resource `{uri}` blob is not base64: {error}"
                                ))
                            })?;
                            EmbeddedResourceResource::BlobResourceContents(
                                BlobResourceContents::new(blob, uri).mime_type(mime_type.clone()),
                            )
                        }
                        _ => {
                            return Err(Error::Message(format!(
                                "resource `{uri}` must contain exactly one of text or blob"
                            )));
                        }
                    };
                    prompt.push(ContentBlock::Resource(EmbeddedResource::new(resource)));
                }
                wire::GatewayInputPart::ResourceLink {
                    name,
                    uri,
                    description,
                    mime_type,
                    size,
                } => {
                    let link = ResourceLink::new(name, uri)
                        .description(description.clone())
                        .mime_type(mime_type.clone())
                        .size(*size);
                    prompt.push(ContentBlock::ResourceLink(link));
                }
            }
        }
    }
    if prompt.is_empty() {
        return Err(Error::Message("ACP prompt input is empty".to_string()));
    }
    Ok(prompt)
}

async fn acp_image_block(
    image: &ImageInput,
    turn: &AcpPeerTurnContext,
) -> psychevo_runtime::Result<ContentBlock> {
    let (source, uri) = match image {
        ImageInput::LocalPath(path) => (path.display().to_string(), None),
        ImageInput::ImageUrl(url) => (
            url.clone(),
            (url.starts_with("http://") || url.starts_with("https://")).then_some(url.clone()),
        ),
    };
    let resolved = resolve_explicit_image_source(&source, &turn.cwd, &turn.home).await?;
    let mut content =
        ImageContent::new(BASE64_STANDARD.encode(&resolved.bytes), resolved.mime_type);
    if let Some(uri) = uri {
        content = content.uri(uri);
    }
    Ok(ContentBlock::Image(content))
}

fn url_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
