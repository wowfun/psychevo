use crate::capabilities::{
    CapabilityCategory, CapabilityContributionRecord, CapabilitySnapshot, CapabilitySnapshotParts,
    source_record,
};
use crate::skills::SelectedSkill;
use crate::store::PromptPrefixRecord;
use crate::types::SelectedAgent;
use psychevo_agent_core::now_ms;
use serde_json::json;

pub(crate) struct ProviderCapabilityInput<'a> {
    pub(crate) provider: &'a str,
    pub(crate) provider_label: &'a str,
    pub(crate) model: &'a str,
    pub(crate) base_url: Option<&'a str>,
    pub(crate) api_key_env: Option<&'a str>,
    pub(crate) reasoning_effort: Option<&'a str>,
    pub(crate) context_limit: Option<u64>,
}

pub(crate) fn add_provider_capability(
    parts: &mut CapabilitySnapshotParts,
    input: ProviderCapabilityInput<'_>,
) {
    let source_id = format!("provider:{}", input.provider);
    parts.push_source(source_record(
        &source_id,
        "provider_adapter",
        input.provider,
        Some(input.provider_label.to_string()),
        "session_snapshot",
        None,
    ));
    let visible_name = format!("{}/{}", input.provider, input.model);
    parts.push_selected(CapabilityContributionRecord {
        id: format!("{source_id}:model:{}", input.model),
        source_id: source_id.clone(),
        category: CapabilityCategory::Provider,
        raw_name: visible_name.clone(),
        visible_name: Some(visible_name),
        exposure: None,
        status: "selected".to_string(),
        reason: None,
        metadata: Some(json!({
            "provider": input.provider,
            "provider_label": input.provider_label,
            "model": input.model,
            "base_url": input.base_url,
            "api_key_env": input.api_key_env,
            "reasoning_effort": input.reasoning_effort,
            "context_limit": input.context_limit,
        })),
    });
}

pub(crate) fn add_agent_capabilities(
    parts: &mut CapabilitySnapshotParts,
    selected_agent: Option<&SelectedAgent>,
    agents_enabled: bool,
    available_count: usize,
    visible_names: Vec<String>,
) {
    let source_id = "runtime:agent-catalog";
    parts.push_source(source_record(
        source_id,
        "runtime",
        "agent catalog",
        None,
        "session_snapshot",
        None,
    ));
    let summary_id = format!("{source_id}:summary");
    parts.contributions.push(CapabilityContributionRecord {
        id: summary_id.clone(),
        source_id: source_id.to_string(),
        category: CapabilityCategory::Agent,
        raw_name: "agent catalog".to_string(),
        visible_name: None,
        exposure: None,
        status: "available_summary".to_string(),
        reason: None,
        metadata: Some(json!({
            "enabled": agents_enabled,
            "available_count": available_count,
            "visible_names": visible_names,
        })),
    });
    parts.push_event(
        "available_summary",
        Some(summary_id),
        Some(source_id.to_string()),
        CapabilityCategory::Agent,
        None,
        None,
    );
    if let Some(agent) = selected_agent {
        parts.push_selected(CapabilityContributionRecord {
            id: format!("{source_id}:agent:{}", agent.name),
            source_id: source_id.to_string(),
            category: CapabilityCategory::Agent,
            raw_name: agent.name.clone(),
            visible_name: Some(agent.name.clone()),
            exposure: None,
            status: "selected".to_string(),
            reason: None,
            metadata: Some(json!({
                "source": agent.source,
                "path": agent.path.as_ref().map(|path| path.display().to_string()),
            })),
        });
    }
}

pub(crate) fn add_skill_capabilities(
    parts: &mut CapabilitySnapshotParts,
    selected_skills: &[SelectedSkill],
    skills_enabled: bool,
    available_count: usize,
    catalog_visible: bool,
) {
    let source_id = "runtime:skill-catalog";
    parts.push_source(source_record(
        source_id,
        "runtime",
        "skill catalog",
        None,
        "session_snapshot",
        None,
    ));
    let summary_id = format!("{source_id}:summary");
    parts.contributions.push(CapabilityContributionRecord {
        id: summary_id.clone(),
        source_id: source_id.to_string(),
        category: CapabilityCategory::Skill,
        raw_name: "skill catalog".to_string(),
        visible_name: None,
        exposure: None,
        status: "available_summary".to_string(),
        reason: None,
        metadata: Some(json!({
            "enabled": skills_enabled,
            "available_count": available_count,
            "catalog_visible": catalog_visible,
        })),
    });
    parts.push_event(
        "available_summary",
        Some(summary_id),
        Some(source_id.to_string()),
        CapabilityCategory::Skill,
        None,
        None,
    );
    for skill in selected_skills {
        parts.push_selected(CapabilityContributionRecord {
            id: format!("{source_id}:skill:{}", skill.name),
            source_id: source_id.to_string(),
            category: CapabilityCategory::Skill,
            raw_name: skill.name.clone(),
            visible_name: Some(skill.name.clone()),
            exposure: None,
            status: "selected".to_string(),
            reason: None,
            metadata: Some(json!({
                "path": skill.path.display().to_string(),
            })),
        });
    }
}

pub(crate) fn add_prompt_prefix_context_capabilities(
    parts: &mut CapabilitySnapshotParts,
    prefix: &PromptPrefixRecord,
) {
    let source_id = format!("prompt-prefix:{}:{}", prefix.session_id, prefix.version);
    parts.push_source(source_record(
        &source_id,
        "prompt_prefix",
        format!("prompt prefix v{}", prefix.version),
        None,
        "session_snapshot",
        None,
    ));
    for slot in &prefix.slots {
        let contribution_id = format!("{source_id}:context:{}", slot.slot);
        parts.push_selected(CapabilityContributionRecord {
            id: contribution_id,
            source_id: source_id.clone(),
            category: CapabilityCategory::Context,
            raw_name: slot.slot.clone(),
            visible_name: None,
            exposure: None,
            status: "selected".to_string(),
            reason: None,
            metadata: Some(json!({
                "slot": slot.slot,
                "tier": slot.tier,
                "semantic_role": slot.semantic_role,
                "provider_role": slot.provider_role,
                "order": slot.order,
                "content_hash": slot.content_hash,
                "source_kind": slot.source_kind,
                "source_name": slot.source_name,
                "source_path": slot.source_path,
            })),
        });
    }
}

pub(crate) fn build_capability_snapshot(
    session_id: &str,
    prompt_prefix: &PromptPrefixRecord,
    mut parts: CapabilitySnapshotParts,
) -> CapabilitySnapshot {
    add_prompt_prefix_context_capabilities(&mut parts, prompt_prefix);
    CapabilitySnapshot::from_parts(
        session_id.to_string(),
        prompt_prefix.version,
        now_ms(),
        parts,
    )
}
