use std::collections::BTreeMap;

use psychevo_ai::{
    LanguageRequest, LanguageTool, Message, ModelDescriptor, OpenAiChatAdapter, ProviderError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OpenAiChatTokenCount {
    pub(crate) encoding: String,
    pub(crate) encoding_source: String,
    pub(crate) encoding_fallback: bool,
    pub(crate) base_policy_tokens: u64,
    pub(crate) developer_prompt_tokens: u64,
    pub(crate) project_context_tokens: u64,
    pub(crate) history_tokens: u64,
    pub(crate) turn_context_tokens: u64,
    pub(crate) current_prompt_tokens: u64,
    pub(crate) system_prompt_tokens: u64,
    pub(crate) system_tools_tokens: u64,
    pub(crate) skills_tokens: u64,
    pub(crate) messages_tokens: u64,
    pub(crate) total_estimated_tokens: u64,
    pub(crate) tool_count: usize,
    pub(crate) role_counts: BTreeMap<String, OpenAiChatRoleTokenCount>,
    pub(crate) project_instruction_context_tokens: u64,
    pub(crate) project_instruction_context_count: usize,
    pub(crate) selected_skill_context_tokens: u64,
    pub(crate) selected_skill_context_count: usize,
    pub(crate) skill_names: Vec<String>,
    pub(crate) skill_entries: Vec<OpenAiChatSkillTokenCount>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OpenAiChatRoleTokenCount {
    pub(crate) count: usize,
    pub(crate) tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OpenAiChatSkillTokenCount {
    pub(crate) name: String,
    pub(crate) tokens: u64,
}

pub(crate) fn count_openai_language_request(
    descriptor: &ModelDescriptor,
    request: &LanguageRequest,
) -> Result<OpenAiChatTokenCount, ProviderError> {
    let encoding = resolve_count_encoding(
        &descriptor.provider_family,
        &descriptor.model_id,
    );
    let Some(enc) = tiktoken::get_encoding(&encoding.name) else {
        return Ok(OpenAiChatTokenCount {
            encoding: "o200k_base".to_string(),
            encoding_source: "fallback".to_string(),
            encoding_fallback: true,
            ..OpenAiChatTokenCount::default()
        });
    };
    let counting = request_context_counting_metadata(request);
    let chat_descriptor = ModelDescriptor {
        protocol_id: "openai_chat".to_string(),
        ..descriptor.clone()
    };
    let mut preview_request = request.clone();
    let unsupported_tools = preview_request
        .tools
        .iter()
        .filter(|tool| !matches!(tool, LanguageTool::Function { .. }))
        .cloned()
        .collect::<Vec<_>>();
    preview_request
        .tools
        .retain(|tool| matches!(tool, LanguageTool::Function { .. }));
    let preview = OpenAiChatAdapter::preview(&chat_descriptor, &preview_request)?;
    let (mut system_tools_tokens, mut tool_count) = preview
        .body
        .get("tools")
        .map(|tools| {
            (
                count_value(enc, tools),
                tools.as_array().map_or(0, Vec::len),
            )
        })
        .unwrap_or((0, 0));
    for tool in unsupported_tools {
        system_tools_tokens =
            system_tools_tokens.saturating_add(count_value(enc, &serde_json::to_value(tool).unwrap_or(Value::Null)));
        tool_count = tool_count.saturating_add(1);
    }

    let mut base_policy_tokens = 0u64;
    let mut developer_prompt_tokens = 0u64;
    let skills_tokens = 0u64;
    let mut project_context_tokens = 0u64;
    let mut history_tokens = 0u64;
    let mut turn_context_tokens = 0u64;
    let mut current_prompt_tokens = 0u64;
    let mut role_counts = BTreeMap::<String, OpenAiChatRoleTokenCount>::new();
    let mut skill_entries = Vec::new();
    let mut transcript_message_count = 0usize;

    for message in &request.messages {
        let mut single = request.clone();
        single.messages = vec![message.clone()];
        single.tools.clear();
        let preview = OpenAiChatAdapter::preview(&chat_descriptor, &single)?;
        let provider_messages = preview
            .body
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let tokens = provider_messages
            .iter()
            .map(|message| count_value(enc, message))
            .sum::<u64>();
        let metadata = message_metadata(message);
        match message {
            Message::System { .. } | Message::Developer { .. } => {
                match metadata
                    .and_then(|value| value.get("prompt_semantic_role"))
                    .and_then(Value::as_str)
                    .unwrap_or("developer_prompt")
                {
                    "base_policy" => {
                        base_policy_tokens = base_policy_tokens.saturating_add(tokens)
                    }
                    _ => {
                        developer_prompt_tokens =
                            developer_prompt_tokens.saturating_add(tokens);
                        if metadata
                            .and_then(|value| value.get("prompt_slot"))
                            .and_then(Value::as_str)
                            == Some("skill_index")
                        {
                            for provider_message in &provider_messages {
                                skill_entries
                                    .extend(skill_entry_token_counts(enc, provider_message));
                            }
                        }
                    }
                }
                continue;
            }
            _ => {}
        }

        let context_category = metadata
            .and_then(|value| value.get("context_category"))
            .and_then(Value::as_str);
        if context_category == Some("project_context") {
            project_context_tokens = project_context_tokens.saturating_add(tokens);
        } else if context_category == Some("turn_context") {
            turn_context_tokens = turn_context_tokens.saturating_add(tokens);
        } else if transcript_message_count < counting.previous_message_count {
            history_tokens = history_tokens.saturating_add(tokens);
            transcript_message_count = transcript_message_count.saturating_add(1);
        } else {
            current_prompt_tokens = current_prompt_tokens.saturating_add(tokens);
            transcript_message_count = transcript_message_count.saturating_add(1);
        }
        for provider_message in provider_messages {
            let role = normalized_message_role(&provider_message);
            let entry = role_counts.entry(role).or_default();
            entry.count = entry.count.saturating_add(1);
            entry.tokens = entry
                .tokens
                .saturating_add(count_value(enc, &provider_message));
        }
    }

    let system_prompt_tokens = base_policy_tokens.saturating_add(developer_prompt_tokens);
    let messages_tokens = project_context_tokens
        .saturating_add(history_tokens)
        .saturating_add(turn_context_tokens)
        .saturating_add(current_prompt_tokens);
    let selected_skill_context_tokens = turn_context_tokens;
    let project_instruction_context_tokens = project_context_tokens;
    let total_estimated_tokens = system_prompt_tokens
        .saturating_add(system_tools_tokens)
        .saturating_add(messages_tokens);
    Ok(OpenAiChatTokenCount {
        encoding: encoding.name,
        encoding_source: encoding.source,
        encoding_fallback: encoding.fallback,
        base_policy_tokens,
        developer_prompt_tokens,
        project_context_tokens,
        history_tokens,
        turn_context_tokens,
        current_prompt_tokens,
        system_prompt_tokens,
        system_tools_tokens,
        skills_tokens,
        messages_tokens,
        total_estimated_tokens,
        tool_count,
        role_counts,
        project_instruction_context_tokens,
        project_instruction_context_count: counting.project_instruction_context_message_count,
        selected_skill_context_tokens,
        selected_skill_context_count: counting.selected_skill_context_message_count,
        skill_names: counting.skill_names,
        skill_entries,
    })
}

fn message_metadata(message: &Message) -> Option<&Value> {
    let extensions = match message {
        Message::System { extensions, .. }
        | Message::Developer { extensions, .. }
        | Message::User { extensions, .. }
        | Message::Tool { extensions, .. } => extensions,
        Message::Assistant { message } => &message.extensions,
    };
    extensions.get("psychevo")
}

#[derive(Default)]
struct RequestContextCountingMetadata {
    previous_message_count: usize,
    project_instruction_context_message_count: usize,
    selected_skill_context_message_count: usize,
    skill_names: Vec<String>,
}

fn request_context_counting_metadata(
    request: &LanguageRequest,
) -> RequestContextCountingMetadata {
    let Some(value) = request
        .extensions
        .get("psychevo")
        .and_then(|value| value.get("context_counting"))
    else {
        return RequestContextCountingMetadata::default();
    };
    RequestContextCountingMetadata {
        previous_message_count: value
            .get("previous_message_count")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize,
        project_instruction_context_message_count: value
            .get("project_instruction_context_message_count")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize,
        selected_skill_context_message_count: value
            .get("selected_skill_context_message_count")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize,
        skill_names: value
            .get("skill_names")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
    }
}

struct CountEncoding {
    name: String,
    source: String,
    fallback: bool,
}

fn resolve_count_encoding(provider: &str, model: &str) -> CountEncoding {
    if let Some(name) = tiktoken::model_to_encoding(model) {
        return CountEncoding {
            name: name.to_string(),
            source: "model".to_string(),
            fallback: false,
        };
    }
    let provider = provider.to_lowercase();
    let model = model.to_lowercase();
    let guessed = if provider.contains("qwen")
        || provider.contains("dashscope")
        || model.contains("qwen")
        || model.contains("qwq")
    {
        Some("qwen2")
    } else if provider.contains("deepseek") || model.contains("deepseek") {
        Some("deepseek_v3")
    } else if provider.contains("llama") || model.contains("llama") {
        Some("llama3")
    } else if provider.contains("mistral") || model.contains("mistral") {
        Some("mistral_v3")
    } else if provider.contains("openai")
        || provider.contains("openrouter")
        || model.starts_with("gpt-")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
    {
        Some("o200k_base")
    } else {
        None
    };
    if let Some(name) = guessed
        && tiktoken::get_encoding(name).is_some()
    {
        return CountEncoding {
            name: name.to_string(),
            source: "provider_model_guess".to_string(),
            fallback: false,
        };
    }
    CountEncoding {
        name: "o200k_base".to_string(),
        source: "fallback".to_string(),
        fallback: true,
    }
}

fn count_value(enc: &tiktoken::CoreBpe, value: &Value) -> u64 {
    serde_json::to_string(value)
        .map(|text| enc.count(&text) as u64)
        .unwrap_or(0)
}

fn normalized_message_role(message: &Value) -> String {
    match message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
    {
        "tool_result" => "tool".to_string(),
        other => other.to_string(),
    }
}

fn skill_entry_token_counts(
    enc: &tiktoken::CoreBpe,
    provider_message: &Value,
) -> Vec<OpenAiChatSkillTokenCount> {
    let Some(content) = provider_message.get("content").and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut rest = content;
    let mut entries = Vec::new();
    while let Some(start_index) = rest.find("<skill>") {
        rest = &rest[start_index + "<skill>".len()..];
        let Some(end_index) = rest.find("</skill>") else {
            break;
        };
        let entry = &rest[..end_index];
        if let Some(name) = skill_entry_name(entry) {
            entries.push(OpenAiChatSkillTokenCount {
                name: name.to_string(),
                tokens: enc.count(entry) as u64,
            });
        }
        rest = &rest[end_index + "</skill>".len()..];
    }
    entries
}

fn skill_entry_name(entry: &str) -> Option<&str> {
    let start = entry.find("<name>")? + "<name>".len();
    let end = entry[start..].find("</name>")? + start;
    let name = entry[start..end].trim();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use psychevo_ai::{
        Capability, LanguageTool, TextContent, ToolDeclaration,
    };
    use serde_json::json;

    fn message_with_metadata(
        text: &str,
        metadata: Value,
    ) -> psychevo_ai::Message {
        psychevo_ai::Message::System {
            content: vec![TextContent::new(text)],
            extensions: BTreeMap::from([("psychevo".to_string(), metadata)]),
        }
    }

    fn user_with_category(
        text: &str,
        category: Option<&str>,
    ) -> psychevo_ai::Message {
        psychevo_ai::Message::User {
            content: vec![psychevo_ai::UserContent::Text(TextContent::new(text))],
            extensions: category
                .map(|category| {
                    BTreeMap::from([(
                        "psychevo".to_string(),
                        json!({"context_category": category}),
                    )])
                })
                .unwrap_or_default(),
        }
    }

    #[test]
    fn product_owned_counter_splits_context_categories_from_typed_request() {
        let descriptor = ModelDescriptor {
            deployment_id: "deepseek".to_string(),
            provider_family: "deepseek".to_string(),
            capability: Capability::Language,
            model_id: "deepseek-chat".to_string(),
            protocol_id: "openai_chat".to_string(),
        };
        let request = LanguageRequest {
            messages: vec![
                message_with_metadata(
                    "mode",
                    json!({
                        "prompt_slot": "base/mode",
                        "prompt_semantic_role": "base_policy",
                    }),
                ),
                message_with_metadata(
                    "<skill><name>alpha</name><description>longer helper</description></skill>",
                    json!({
                        "prompt_slot": "skill_index",
                        "prompt_semantic_role": "developer_prompt",
                    }),
                ),
                user_with_category("project instructions", Some("project_context")),
                user_with_category("previous", None),
                user_with_category("selected skill body", Some("turn_context")),
                psychevo_ai::Message::assistant("ok"),
            ],
            tools: vec![LanguageTool::from(ToolDeclaration::new(
                "read",
                "read file",
                json!({"type": "object"}),
            ))],
            extensions: BTreeMap::from([(
                "psychevo".to_string(),
                json!({
                    "context_counting": {
                        "previous_message_count": 1,
                        "project_instruction_context_message_count": 1,
                        "selected_skill_context_message_count": 1,
                        "skill_names": ["alpha"],
                    }
                }),
            )]),
            ..LanguageRequest::default()
        };

        let count =
            count_openai_language_request(&descriptor, &request).expect("count");
        assert!(count.base_policy_tokens > 0);
        assert!(count.developer_prompt_tokens > 0);
        assert!(count.system_tools_tokens > 0);
        assert!(count.project_context_tokens > 0);
        assert!(count.history_tokens > 0);
        assert!(count.turn_context_tokens > 0);
        assert!(count.current_prompt_tokens > 0);
        assert_eq!(count.tool_count, 1);
        assert_eq!(count.role_counts["user"].count, 3);
        assert_eq!(count.role_counts["assistant"].count, 1);
        assert_eq!(count.skill_names, vec!["alpha"]);
        assert_eq!(count.skill_entries[0].name, "alpha");
        assert_eq!(count.encoding, "deepseek_v3");
    }
}
