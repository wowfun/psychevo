#[path = "commands/prompt_submission.rs"]
mod prompt_submission;
pub(crate) use prompt_submission::SubmittedSlashInput;
#[path = "commands/formatting.rs"]
mod formatting;
#[path = "commands/slash_dispatch.rs"]
mod slash_dispatch;
pub(crate) use formatting::{
    fork_prompt_marker, format_compaction_result, format_skill_mutation_result,
    fullscreen_context_bar_width, json_string, json_string_array, mission_command_args,
    normalize_dynamic_skill_name, normalize_submitted_slash_echo,
    resolve_tui_turn_admission_target, skill_args_without_scope, skill_option_value,
    skill_prompt_marker, skill_scope_from_args, slash_command_echo,
};
