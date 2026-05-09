#[derive(Clone)]
pub struct AgentLoopRequest {
    pub model_provider: String,
    pub model: String,
    pub generation_metadata: Value,
    pub system_instructions: Vec<String>,
    pub previous_messages: Vec<Message>,
    pub context_messages: Vec<Message>,
    pub prompt_messages: Vec<Message>,
    pub tools: Vec<Arc<dyn ToolBinding>>,
    pub max_turns: usize,
}

#[derive(Debug, Clone)]
pub struct AgentCompletion {
    pub outcome: Outcome,
    pub messages: Vec<Message>,
}
