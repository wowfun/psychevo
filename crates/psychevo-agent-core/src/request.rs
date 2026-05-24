#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptInstruction {
    pub slot: String,
    pub tier: String,
    pub semantic_role: String,
    pub provider_role: String,
    pub order: usize,
    pub content: String,
    pub content_hash: String,
    pub source_kind: Option<String>,
    pub source_name: Option<String>,
    pub source_path: Option<String>,
}

impl PromptInstruction {
    pub fn inline_system(
        slot: impl Into<String>,
        order: usize,
        content: impl Into<String>,
    ) -> Self {
        Self {
            slot: slot.into(),
            tier: "turn".to_string(),
            semantic_role: "system".to_string(),
            provider_role: "system".to_string(),
            order,
            content: content.into(),
            content_hash: String::new(),
            source_kind: Some("runtime".to_string()),
            source_name: None,
            source_path: None,
        }
    }

    pub fn to_provider_value(&self) -> Value {
        let provider_role = match self.provider_role.as_str() {
            "" => "system",
            role => role,
        };
        json!({
            "role": provider_role,
            "content": self.content,
            "metadata": {
                "prompt_slot": self.slot,
                "prompt_slot_tier": self.tier,
                "prompt_semantic_role": self.semantic_role,
                "prompt_content_hash": self.content_hash,
                "prompt_order": self.order,
                "source_kind": self.source_kind,
                "source_name": self.source_name,
                "source_path": self.source_path,
            }
        })
    }
}

#[derive(Clone)]
pub struct AgentLoopRequest {
    pub model_provider: String,
    pub model: String,
    pub generation_metadata: Value,
    pub prompt_instructions: Vec<PromptInstruction>,
    pub turn_prompt_instructions: Vec<PromptInstruction>,
    pub previous_messages: Vec<Message>,
    pub context_messages: Vec<Message>,
    pub prefix_contextual_user_messages: Vec<ContextualUserMessage>,
    pub turn_contextual_user_messages: Vec<ContextualUserMessage>,
    pub prompt_messages: Vec<Message>,
    pub tools: Vec<Arc<dyn ToolBinding>>,
    pub max_turns: usize,
}

#[derive(Debug, Clone)]
pub struct AgentCompletion {
    pub outcome: Outcome,
    pub messages: Vec<Message>,
    pub terminal_reason: Option<TerminalReason>,
}
