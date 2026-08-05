use psychevo::agents::{AgentCatalog, AgentDiscoveryOptions, discover_agents};
use psychevo::command_registry::{DynamicSlashCommand, skill_prompt_marker};
use psychevo::skills::{SkillCatalog, SkillDiscoveryOptions, discover_skills, list_skill_bundles};

use super::agents::materialize_local_acp_backends;
use super::binding::WebState;
use super::scope_session::ResolvedScope;

mod models;
mod observability;
mod workbench;

pub(super) use models::{
    model_assignment_set_value, model_provider_catalog_value, model_provider_save_value,
    model_settings_value, model_state_read_value, model_state_set_value,
};
pub(super) use observability::{context_read_value, observability_read_value, usage_read_value};
#[cfg(test)]
pub(super) use workbench::display_relative_to_home;
pub(super) use workbench::{
    display_cwd, native_runtime_mode_option, session_control_agent, settings_read_value,
    update_session_agent_setting, web_search_settings_update_value, web_search_settings_value,
};

pub(super) fn discover_gateway_agents(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo::Result<AgentCatalog> {
    materialize_local_acp_backends(state, scope)?;
    discover_agents(&AgentDiscoveryOptions {
        home: state.inner.home.clone(),
        cwd: scope.cwd.clone(),
        env: state.inner.inherited_env.clone(),
        explicit_inputs: Vec::new(),
        no_agents: false,
    })
}

pub(super) fn discover_gateway_skills(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo::Result<SkillCatalog> {
    discover_skills(&SkillDiscoveryOptions {
        home: state.inner.home.clone(),
        cwd: scope.cwd.clone(),
        config_path: state.inner.config_path.clone(),
        env: state.inner.inherited_env.clone(),
        explicit_inputs: Vec::new(),
        additional_roots: Vec::new(),
        no_skills: false,
    })
}

pub(super) fn dynamic_slash_commands(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo::Result<Vec<DynamicSlashCommand>> {
    let mut commands = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for bundle in list_skill_bundles(&state.inner.home, &scope.cwd)? {
        if seen.insert(bundle.slug.clone()) {
            commands.push(DynamicSlashCommand {
                name: bundle.slug.clone(),
                summary: bundle.description,
                prompt: skill_prompt_marker(&bundle.slug, ""),
            });
        }
    }
    for skill in discover_gateway_skills(state, scope)?.skills {
        if skill.disable_model_invocation || !skill.supported_on_current_platform {
            continue;
        }
        if seen.insert(skill.name.clone()) {
            commands.push(DynamicSlashCommand {
                name: skill.name.clone(),
                summary: skill.description,
                prompt: skill_prompt_marker(&skill.name, ""),
            });
        }
    }
    Ok(commands)
}
