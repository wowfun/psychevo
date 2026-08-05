use psychevo::application::{StoredEditableInputEnvelope, StoredEditableInputPart};
use psychevo::{ImageInput, PromptDisplayMetadata};

use super::agent_session::{AgentErrorStage, agent_session_error};
use psychevo_gateway_protocol::source::{GatewayImageInput, GatewayInputPart, SourceKey};

pub(super) fn thread_key(thread_id: &str) -> String {
    format!("thread:{thread_id}")
}

pub(super) fn source_key_key(source_key: &SourceKey) -> String {
    format!("source:{}", source_key.0)
}

pub(super) fn framework_input_parts(
    input: &[GatewayInputPart],
) -> psychevo::Result<(String, Vec<ImageInput>, PromptDisplayMetadata, bool)> {
    let mut prompt_parts = Vec::new();
    let mut image_inputs = Vec::new();
    let mut editable_parts = Vec::new();
    let mut editable_text_parts = Vec::new();
    let mut has_structured_input = false;
    for part in input {
        match part {
            GatewayInputPart::Text { text } => {
                prompt_parts.push(text.clone());
                editable_text_parts.push(text.clone());
                editable_parts.push(StoredEditableInputPart::Text { text: text.clone() });
            }
            GatewayInputPart::Context {
                text,
                visible_to_model,
                ..
            } if *visible_to_model => prompt_parts.push(text.clone()),
            GatewayInputPart::Context { .. } => {}
            GatewayInputPart::Image { input } => {
                let image_block_index = image_inputs.len();
                image_inputs.push(gateway_image_input_into_runtime(input.clone()));
                editable_parts.push(StoredEditableInputPart::Image { image_block_index });
            }
            GatewayInputPart::Resource { text, blob, .. } => {
                if text.is_some() == blob.is_some() {
                    return Err(agent_session_error(
                        "invalid_input",
                        AgentErrorStage::Delivery,
                        "user_action",
                        "not_delivered",
                        "A resource input must contain exactly one of `text` or `blob`.",
                        None,
                    ));
                }
                has_structured_input = true;
            }
            GatewayInputPart::ResourceLink { name, uri, .. } => {
                if name.trim().is_empty() || uri.trim().is_empty() {
                    return Err(agent_session_error(
                        "invalid_input",
                        AgentErrorStage::Delivery,
                        "user_action",
                        "not_delivered",
                        "A resource link requires non-empty `name` and `uri`.",
                        None,
                    ));
                }
                has_structured_input = true;
            }
        }
    }
    let prompt = prompt_parts.join("\n");
    let prompt_display = PromptDisplayMetadata {
        content_text: editable_text_parts.join("\n"),
        attachments: Vec::new(),
        editable_input: Some(StoredEditableInputEnvelope {
            version: 1,
            parts: editable_parts,
        }),
    };
    Ok((prompt, image_inputs, prompt_display, has_structured_input))
}

fn gateway_image_input_into_runtime(input: GatewayImageInput) -> ImageInput {
    match input {
        GatewayImageInput::LocalPath { path } => ImageInput::LocalPath(path.into()),
        GatewayImageInput::Url { url } => ImageInput::ImageUrl(url),
    }
}
