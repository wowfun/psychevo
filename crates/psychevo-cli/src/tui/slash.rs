#[path = "slash/config_help.rs"]
mod config_help;
#[cfg(test)]
pub(crate) use config_help::base_slash_menu_items;
#[cfg(test)]
pub(crate) use config_help::format_slash_help;
pub(crate) use config_help::{
    EffectiveSlashConfig, OBSOLETE_SLASH_COMMAND_TOKENS, OLD_DYNAMIC_SKILL_PREFIX, SlashCommand,
    SlashHelpSections, SlashMenuItem, SlashShortcutMatch, TuiExportOptions, TuiShareOptions,
    VARIANTS, configured_slash_menu_items, format_slash_help_with_config,
    parse_effective_slash_config, slash_help_sections_with_config,
};
#[path = "slash/parser_menu.rs"]
mod parser_menu;
pub(crate) use parser_menu::{
    TuiSlashParse, canonical_command_token, normalize_key_code, normalized_modifiers,
    parse_slash_command_with_config, parse_tui_slash_with_config, slash_menu_items_from,
    slash_prefix_menu_items_from, split_command_token, validate_model_spec, validate_variant,
};
#[cfg(test)]
pub(crate) use parser_menu::{parse_slash_command, slash_menu_items, slash_prefix_menu_items};
#[cfg(test)]
#[path = "slash/tests.rs"]
mod tests;
