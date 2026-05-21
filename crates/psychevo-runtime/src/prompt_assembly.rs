use std::sync::Arc;

use psychevo_agent_core::ToolBinding;
use psychevo_agent_core::{ContextualUserBlock, ContextualUserMessage, PromptInstruction};
use serde_json::{Value, json};

use crate::agents::{
    AgentDefinition, AgentInvocationRole, format_agents_for_prompt,
    format_selected_agent_instruction,
};
use crate::project_instructions::ProjectInstructionFragment;
use crate::prompt_templates;
use crate::skills::{Skill, SkillContextFragment, format_skills_for_prompt};
use crate::store::{ContextEvidenceInput, PromptPrefixRecord, PromptPrefixSlotRecord};
use crate::tools::mode_instruction_for_tool_availability;
use crate::types::{ModelCapabilities, RunMode};

pub(crate) const PROMPT_PREFIX_NOTICE_METADATA_KEY: &str = "prompt_prefix_notice";

#[derive(Debug, Clone)]
pub(crate) struct MainPromptAssembly {
    pub(crate) prompt_instructions: Vec<PromptInstruction>,
    pub(crate) prefix_contextual_user_messages: Vec<ContextualUserMessage>,
    pub(crate) prefix_slots: Vec<PromptPrefixSlotRecord>,
    pub(crate) prefix_hash: String,
}

pub(crate) struct PromptPrefixRecordInput<'a> {
    pub(crate) session_id: &'a str,
    pub(crate) provider: &'a str,
    pub(crate) model: &'a str,
    pub(crate) prefix_hash: String,
    pub(crate) tool_declarations_hash: String,
    pub(crate) invalidation_reason: Option<String>,
    pub(crate) slots: Vec<PromptPrefixSlotRecord>,
    pub(crate) metadata: Option<Value>,
}

struct PrefixSlotInput {
    slot: String,
    tier: String,
    semantic_role: String,
    provider_role: String,
    order: usize,
    content: String,
    source_kind: Option<String>,
    source_name: Option<String>,
    source_path: Option<String>,
}

impl PrefixSlotInput {
    fn new(
        slot: impl Into<String>,
        tier: impl Into<String>,
        semantic_role: impl Into<String>,
        provider_role: impl Into<String>,
        order: usize,
        content: impl Into<String>,
    ) -> Self {
        Self {
            slot: slot.into(),
            tier: tier.into(),
            semantic_role: semantic_role.into(),
            provider_role: provider_role.into(),
            order,
            content: content.into(),
            source_kind: None,
            source_name: None,
            source_path: None,
        }
    }

    fn source(
        mut self,
        source_kind: impl Into<String>,
        source_name: impl Into<String>,
        source_path: Option<String>,
    ) -> Self {
        self.source_kind = Some(source_kind.into());
        self.source_name = Some(source_name.into());
        self.source_path = source_path;
        self
    }
}

pub(crate) fn assemble_main_prompt_prefix(
    mode: RunMode,
    selected_agent: Option<&AgentDefinition>,
    agents: &[AgentDefinition],
    skills: &[Skill],
    project_instruction_fragments: &[ProjectInstructionFragment],
    capabilities: &ModelCapabilities,
    tools_available: bool,
) -> MainPromptAssembly {
    let developer_role = developer_provider_role(capabilities);
    let mut order = 0usize;
    let mut prompt_instructions = Vec::new();
    let mut prefix_slots = Vec::new();

    let mode_slot = prefix_slot(
        PrefixSlotInput::new(
            "base/mode",
            "base",
            "base_policy",
            "system",
            order,
            mode_instruction_for_tool_availability(mode, tools_available),
        )
        .source("runtime", "mode", None),
    );
    order += 1;
    prompt_instructions.push(instruction_from_slot(&mode_slot));
    prefix_slots.push(mode_slot);

    if let Some(agent) = selected_agent {
        let selected_slot = prefix_slot(
            PrefixSlotInput::new(
                "selected_main_agent",
                "prefix",
                "developer_prompt",
                developer_role,
                order,
                format_selected_agent_instruction(agent, AgentInvocationRole::Main),
            )
            .source(
                "agent_definition",
                agent.name.as_str(),
                agent
                    .file_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
            ),
        );
        order += 1;
        prompt_instructions.push(instruction_from_slot(&selected_slot));
        prefix_slots.push(selected_slot);
    }

    let agents_prompt = format_agents_for_prompt(agents);
    if !agents_prompt.trim().is_empty() {
        let agent_catalog_slot = prefix_slot(
            PrefixSlotInput::new(
                "agent_catalog",
                "prefix",
                "developer_prompt",
                developer_role,
                order,
                agents_prompt,
            )
            .source("agent_catalog", "active_agents", None),
        );
        order += 1;
        prompt_instructions.push(instruction_from_slot(&agent_catalog_slot));
        prefix_slots.push(agent_catalog_slot);
    }

    let skills_prompt = format_skills_for_prompt(skills);
    if !skills_prompt.trim().is_empty() {
        let skill_index_slot = prefix_slot(
            PrefixSlotInput::new(
                "skill_index",
                "prefix",
                "developer_prompt",
                developer_role,
                order,
                skills_prompt,
            )
            .source("skill_catalog", "active_skills", None),
        );
        order += 1;
        prompt_instructions.push(instruction_from_slot(&skill_index_slot));
        prefix_slots.push(skill_index_slot);
    }

    for (index, fragment) in project_instruction_fragments.iter().enumerate() {
        let project_slot = prefix_slot(
            PrefixSlotInput::new(
                format!("project_context:{index}"),
                "prefix",
                "developer_prompt",
                developer_role,
                order + index,
                format_project_instruction_prompt(fragment),
            )
            .source(
                "project_instruction",
                fragment.source_name.as_str(),
                Some(fragment.source_path.display().to_string()),
            ),
        );
        prompt_instructions.push(instruction_from_slot(&project_slot));
        prefix_slots.push(project_slot);
    }

    let prefix_hash = prefix_hash(&prefix_slots);
    MainPromptAssembly {
        prompt_instructions,
        prefix_contextual_user_messages: Vec::new(),
        prefix_slots,
        prefix_hash,
    }
}

pub(crate) fn assemble_child_prompt_prefix(
    mode: RunMode,
    selected_agent: &AgentDefinition,
    capabilities: &ModelCapabilities,
    tools_available: bool,
) -> MainPromptAssembly {
    let developer_role = developer_provider_role(capabilities);
    let mut order = 0usize;
    let mut prompt_instructions = Vec::new();
    let mut prefix_slots = Vec::new();

    let mode_slot = prefix_slot(
        PrefixSlotInput::new(
            "base/mode",
            "base",
            "base_policy",
            "system",
            order,
            mode_instruction_for_tool_availability(mode, tools_available),
        )
        .source("runtime", "mode", None),
    );
    order += 1;
    prompt_instructions.push(instruction_from_slot(&mode_slot));
    prefix_slots.push(mode_slot);

    let selected_slot = prefix_slot(
        PrefixSlotInput::new(
            "selected_child_agent",
            "prefix",
            "developer_prompt",
            developer_role,
            order,
            format_selected_agent_instruction(selected_agent, AgentInvocationRole::Subagent),
        )
        .source(
            "agent_definition",
            selected_agent.name.as_str(),
            selected_agent
                .file_path
                .as_ref()
                .map(|path| path.display().to_string()),
        ),
    );
    order += 1;
    prompt_instructions.push(instruction_from_slot(&selected_slot));
    prefix_slots.push(selected_slot);

    let control_slot = prefix_slot(
        PrefixSlotInput::new(
            "child_agent_control",
            "prefix",
            "developer_prompt",
            developer_role,
            order,
            prompt_templates::child_agent_control(),
        )
        .source("runtime", "child_agent_control", None),
    );
    prompt_instructions.push(instruction_from_slot(&control_slot));
    prefix_slots.push(control_slot);

    let prefix_hash = prefix_hash(&prefix_slots);
    MainPromptAssembly {
        prompt_instructions,
        prefix_contextual_user_messages: Vec::new(),
        prefix_slots,
        prefix_hash,
    }
}

pub(crate) fn assembly_from_prefix_record(record: &PromptPrefixRecord) -> MainPromptAssembly {
    let prompt_instructions = record
        .slots
        .iter()
        .filter(|slot| slot.provider_role != "user")
        .map(instruction_from_slot)
        .collect::<Vec<_>>();
    let project_blocks = record
        .slots
        .iter()
        .filter(|slot| slot.provider_role == "user" && slot.semantic_role == "project_context")
        .map(|slot| {
            ContextualUserBlock::new(
                slot.source_kind
                    .clone()
                    .unwrap_or_else(|| "project_instruction".to_string()),
                slot.source_name.clone(),
                slot.source_path.clone(),
                slot.content.clone(),
            )
        })
        .collect::<Vec<_>>();
    let prefix_contextual_user_messages = if project_blocks.is_empty() {
        Vec::new()
    } else {
        vec![ContextualUserMessage::new_with_category(
            "project_instructions",
            "project_context",
            project_blocks,
        )]
    };
    MainPromptAssembly {
        prompt_instructions,
        prefix_contextual_user_messages,
        prefix_slots: record.slots.clone(),
        prefix_hash: record.prefix_hash.clone(),
    }
}

pub(crate) fn turn_required_agent_instruction(
    required_agent_mentions: &[String],
    capabilities: &ModelCapabilities,
    order: usize,
) -> Option<PromptInstruction> {
    if required_agent_mentions.is_empty() {
        return None;
    }
    let content = prompt_templates::required_agent_calls(&required_agent_mentions.join(", "));
    Some(turn_instruction(
        "required_agent_calls",
        "developer_prompt",
        developer_provider_role(capabilities),
        order,
        content,
        Some("user_prompt_hint"),
        Some("required_agent_mentions"),
    ))
}

pub(crate) fn turn_prefix_notice_instruction(
    notice: &str,
    capabilities: &ModelCapabilities,
    order: usize,
) -> Option<PromptInstruction> {
    let notice = notice.trim();
    if notice.is_empty() {
        return None;
    }
    Some(turn_instruction(
        "prefix_reload_notice",
        "developer_prompt",
        developer_provider_role(capabilities),
        order,
        notice,
        Some("session_metadata"),
        Some(PROMPT_PREFIX_NOTICE_METADATA_KEY),
    ))
}

pub(crate) fn skill_contextual_user_messages(
    skill_fragments: &[SkillContextFragment],
) -> Vec<ContextualUserMessage> {
    skill_fragments
        .iter()
        .enumerate()
        .map(|(index, fragment)| {
            ContextualUserMessage::new_with_category(
                selected_skill_provider_group(index, &fragment.name),
                "turn_context",
                vec![ContextualUserBlock::new(
                    "selected_skill",
                    Some(fragment.name.clone()),
                    Some(fragment.path.display().to_string()),
                    fragment.content.clone(),
                )],
            )
        })
        .collect()
}

pub(crate) fn selected_skill_provider_group(index: usize, name: &str) -> String {
    format!("selected_skill:{index}:{name}")
}

pub(crate) fn prompt_prefix_record(input: PromptPrefixRecordInput<'_>) -> PromptPrefixRecord {
    PromptPrefixRecord {
        session_id: input.session_id.to_string(),
        version: 0,
        created_at_ms: psychevo_agent_core::now_ms(),
        provider: input.provider.to_string(),
        model: input.model.to_string(),
        prefix_hash: input.prefix_hash,
        tool_declarations_hash: input.tool_declarations_hash,
        invalidation_reason: input.invalidation_reason,
        slots: input.slots,
        metadata: input.metadata,
    }
}

pub(crate) fn context_evidence_for_request(
    prompt_instructions: &[PromptInstruction],
    turn_prompt_instructions: &[PromptInstruction],
    prefix_contextual_user_messages: &[ContextualUserMessage],
    skill_fragments: &[SkillContextFragment],
) -> Vec<ContextEvidenceInput> {
    let mut evidence = Vec::new();
    for (index, instruction) in prompt_instructions
        .iter()
        .chain(turn_prompt_instructions.iter())
        .enumerate()
    {
        evidence.push(ContextEvidenceInput {
            role: instruction.provider_role.clone(),
            source_kind: instruction
                .source_kind
                .clone()
                .unwrap_or_else(|| "prompt_instruction".to_string()),
            source_name: instruction
                .source_name
                .clone()
                .or_else(|| Some(instruction.slot.clone())),
            source_path: instruction.source_path.clone(),
            provider_group: Some(format!("{}_prompt_instructions", instruction.tier)),
            provider_block_index: Some(index as i64),
            context_kind: Some(instruction.semantic_role.clone()),
            content_text: instruction.content.clone(),
            metadata: Some(json!({
                "slot": instruction.slot.clone(),
                "tier": instruction.tier.clone(),
                "semantic_role": instruction.semantic_role.clone(),
                "content_hash": instruction.content_hash.clone(),
                "order": instruction.order,
            })),
        });
    }
    for message in prefix_contextual_user_messages {
        for (index, block) in message.blocks.iter().enumerate() {
            evidence.push(ContextEvidenceInput {
                role: "user".to_string(),
                source_kind: block.kind.clone(),
                source_name: block.source_name.clone(),
                source_path: block.source_path.clone(),
                provider_group: Some(message.provider_group.clone()),
                provider_block_index: Some(index as i64),
                context_kind: Some(block.kind.clone()),
                content_text: block.text.clone(),
                metadata: Some(json!({
                    "context_category": message.context_category.clone(),
                    "hidden": block.hidden,
                })),
            });
        }
    }
    for (index, fragment) in skill_fragments.iter().enumerate() {
        evidence.push(ContextEvidenceInput {
            role: "user".to_string(),
            source_kind: "selected_skill".to_string(),
            source_name: Some(fragment.name.clone()),
            source_path: Some(fragment.path.display().to_string()),
            provider_group: Some(selected_skill_provider_group(index, &fragment.name)),
            provider_block_index: Some(0),
            context_kind: Some("selected_skill".to_string()),
            content_text: fragment.content.clone(),
            metadata: Some(json!({
                "base_dir": fragment.base_dir.display().to_string(),
            })),
        });
    }
    evidence
}

pub(crate) fn tool_declarations_hash(tools: &[Arc<dyn ToolBinding>]) -> String {
    let declarations = tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name(),
                "description": tool.description(),
                "parameters": tool.parameters(),
            })
        })
        .collect::<Vec<_>>();
    stable_hash_hex(&serde_json::to_string(&declarations).unwrap_or_default())
}

pub(crate) fn stable_hash_hex(text: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn format_project_instruction_prompt(fragment: &ProjectInstructionFragment) -> String {
    prompt_templates::project_context(&fragment.content)
}

pub(crate) fn developer_provider_role(capabilities: &ModelCapabilities) -> &'static str {
    if capabilities.developer_role == Some(true) {
        "developer"
    } else {
        "system"
    }
}

fn prefix_slot(input: PrefixSlotInput) -> PromptPrefixSlotRecord {
    let content = input.content;
    PromptPrefixSlotRecord {
        slot: input.slot,
        tier: input.tier,
        semantic_role: input.semantic_role,
        provider_role: input.provider_role,
        order: input.order,
        content_hash: stable_hash_hex(&content),
        content,
        source_kind: input.source_kind,
        source_name: input.source_name,
        source_path: input.source_path,
    }
}

fn turn_instruction(
    slot: impl Into<String>,
    semantic_role: impl Into<String>,
    provider_role: impl Into<String>,
    order: usize,
    content: impl Into<String>,
    source_kind: Option<&str>,
    source_name: Option<&str>,
) -> PromptInstruction {
    let content = content.into();
    PromptInstruction {
        slot: slot.into(),
        tier: "turn".to_string(),
        semantic_role: semantic_role.into(),
        provider_role: provider_role.into(),
        order,
        content_hash: stable_hash_hex(&content),
        content,
        source_kind: source_kind.map(str::to_string),
        source_name: source_name.map(str::to_string),
        source_path: None,
    }
}

fn instruction_from_slot(slot: &PromptPrefixSlotRecord) -> PromptInstruction {
    PromptInstruction {
        slot: slot.slot.clone(),
        tier: slot.tier.clone(),
        semantic_role: slot.semantic_role.clone(),
        provider_role: slot.provider_role.clone(),
        order: slot.order,
        content: slot.content.clone(),
        content_hash: slot.content_hash.clone(),
        source_kind: slot.source_kind.clone(),
        source_name: slot.source_name.clone(),
        source_path: slot.source_path.clone(),
    }
}

fn prefix_hash(slots: &[PromptPrefixSlotRecord]) -> String {
    stable_hash_hex(&serde_json::to_string(slots).unwrap_or_default())
}
