#[path = "parsing/parser.rs"]
mod parser;

pub use parser::{
    CLI_COMMANDS, CUSTOM_SKILL_COMMAND, available_slash_commands_for_surface,
    dynamic_slash_command_effect, fork_prompt_marker, mission_prompt_marker,
    normalize_dynamic_skill_name, parse_mission_args, parse_slash_command_line,
    skill_prompt_marker, slash_command_spec, slash_invocation_effect, supported_by_capabilities,
};

#[cfg(test)]
#[path = "parsing/tests.rs"]
mod tests;
