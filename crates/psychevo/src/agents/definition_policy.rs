mod tools;
mod validation;

pub(super) use tools::{HookedTool, SpawnAgentTool};
#[cfg(test)]
pub(super) use validation::built_in_agent;
pub(super) use validation::{
    agent_allows_tool, agent_catalog_for_policy, agent_policy_allows_agent_catalog,
    agent_policy_allows_skill_catalog, ancestor_compatible_agent_dirs, built_in_agents,
    clamp_agent_spawn_depth, existing_agent_path, home_path, parse_agent_file, split_frontmatter,
};
pub use validation::{parse_agent_definition_text, valid_agent_name};
