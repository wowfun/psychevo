use anyhow::{Result, anyhow};
use psychevo_runtime::split_image_source_argument;

use crate::command_registry::{
    CUSTOM_SKILL_COMMAND, CommandArgumentKind, CommandGroup, CommandStatus, CommandSurface,
    SLASH_COMMANDS, SlashCommandAction, SlashCommandSpec, slash_command_spec,
};

pub(crate) const VARIANTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];
const GENERAL_COMMANDS: &[&str] = &[
    "/status",
    "/context",
    "/model",
    "/sessions",
    "/new",
    "/copy",
    "/undo",
    "/redo",
    "/quit",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlashCommand {
    Help,
    Quit,
    Status,
    New,
    Sessions,
    Usage,
    Context,
    ModelShow,
    VariantSet(String),
    ModeSet(String),
    ThinkingToggle,
    ThinkingSet(bool),
    RawToggle,
    RawSet(bool),
    Copy,
    Image { source: String, prompt: String },
    Rename(String),
    Undo,
    Redo,
    Skills,
    SkillInvoke { name: String, args: String },
    Upcoming(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlashMenuItem {
    pub(crate) command: String,
    pub(crate) description: String,
    pub(crate) upcoming: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlashHelpSections {
    pub(crate) general: Vec<String>,
    pub(crate) commands: Vec<String>,
    pub(crate) custom_commands: Vec<String>,
}

pub(crate) fn base_slash_menu_items() -> Vec<SlashMenuItem> {
    SLASH_COMMANDS
        .iter()
        .map(|spec| SlashMenuItem {
            command: spec.canonical.to_string(),
            description: spec.summary.to_string(),
            upcoming: spec.status == CommandStatus::Upcoming,
        })
        .collect()
}

pub(crate) fn format_slash_help(skill_count: Option<usize>) -> String {
    let sections = slash_help_sections(skill_count);
    let mut lines = Vec::new();
    lines.push("General".to_string());
    lines.extend(sections.general);
    lines.push(String::new());
    lines.push("Commands".to_string());
    lines.extend(sections.commands);
    lines.push(String::new());
    lines.push("Custom commands".to_string());
    lines.extend(sections.custom_commands);
    lines.join("\n")
}

pub(crate) fn slash_help_sections(skill_count: Option<usize>) -> SlashHelpSections {
    let mut general = vec![
        "Shortcuts".to_string(),
        "Enter - submit".to_string(),
        "Shift+Enter/Ctrl+Enter/Alt+Enter/Ctrl+J - insert newline".to_string(),
        "Tab - complete slash command".to_string(),
        "Esc - close active UI or interrupt running work".to_string(),
        "Ctrl+C/Ctrl+D - quit or copy active selection".to_string(),
        "Ctrl+O - copy latest answer as Markdown".to_string(),
        "Ctrl+B - toggle sidebar".to_string(),
        "Ctrl+T - focus transcript".to_string(),
        "Ctrl+R - search prompt history".to_string(),
        "Up/Down - move cursor or recall history at prompt boundaries".to_string(),
        "PageUp/PageDown - scroll".to_string(),
        String::new(),
        "Common commands".to_string(),
    ];
    for command in GENERAL_COMMANDS {
        if let Some(spec) = slash_command_spec(command)
            && spec.common
            && spec.status == CommandStatus::Active
        {
            general.push(help_command_row(spec));
        }
    }

    let mut commands = Vec::new();
    for spec in SLASH_COMMANDS
        .iter()
        .filter(|spec| spec.group == CommandGroup::Commands)
    {
        debug_assert_eq!(spec.surface, CommandSurface::TuiSlash);
        commands.push(help_command_row(spec));
    }

    let custom_commands = match skill_count {
        Some(count) if count > 0 => vec![format!(
            "{} - {} ({count} available)",
            CUSTOM_SKILL_COMMAND.usage, CUSTOM_SKILL_COMMAND.summary
        )],
        _ => vec!["No custom commands available".to_string()],
    };

    SlashHelpSections {
        general,
        commands,
        custom_commands,
    }
}

fn help_command_row(spec: &SlashCommandSpec) -> String {
    let aliases = if spec.aliases.is_empty() {
        String::new()
    } else {
        format!(" (aliases: {})", spec.aliases.join(", "))
    };
    format!("{} - {}{}", spec.usage, spec.summary, aliases)
}

#[cfg(test)]
pub(crate) fn slash_menu_items(input: &str) -> Vec<SlashMenuItem> {
    slash_menu_items_for(input, &base_slash_menu_items(), MatchMode::Fuzzy)
}

#[cfg(test)]
pub(crate) fn slash_prefix_menu_items(input: &str) -> Vec<SlashMenuItem> {
    slash_menu_items_for(input, &base_slash_menu_items(), MatchMode::Prefix)
}

pub(crate) fn slash_menu_items_from(input: &str, items: &[SlashMenuItem]) -> Vec<SlashMenuItem> {
    slash_menu_items_for(input, items, MatchMode::Fuzzy)
}

pub(crate) fn slash_prefix_menu_items_from(
    input: &str,
    items: &[SlashMenuItem],
) -> Vec<SlashMenuItem> {
    slash_menu_items_for(input, items, MatchMode::Prefix)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchMode {
    Prefix,
    Fuzzy,
}

fn slash_menu_items_for(
    input: &str,
    menu: &[SlashMenuItem],
    mode: MatchMode,
) -> Vec<SlashMenuItem> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') || trimmed.chars().any(char::is_whitespace) {
        return Vec::new();
    }
    let query = trimmed.to_lowercase();
    let mut items = menu
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            slash_match_score(&item.command, &query, mode).map(|score| (score, index, item))
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|(score, index, _)| (*score, *index));
    let mut items = items
        .into_iter()
        .map(|(_, _, item)| item)
        .take(8)
        .cloned()
        .collect::<Vec<_>>();
    if query != "/"
        && let Some(index) = items.iter().position(|item| item.command == query)
    {
        let exact = items.remove(index);
        items.insert(0, exact);
    }
    items
}

fn slash_match_score(command: &str, query: &str, mode: MatchMode) -> Option<u16> {
    if query == "/" {
        return Some(300);
    }
    if command == query {
        return Some(0);
    }
    if command.starts_with(query) {
        return Some(100);
    }
    if mode == MatchMode::Fuzzy
        && let Some(score) = fuzzy_subsequence_score(command, query)
    {
        return Some(200 + score);
    }
    None
}

fn fuzzy_subsequence_score(command: &str, query: &str) -> Option<u16> {
    let command = command.chars().collect::<Vec<_>>();
    let mut last_match = 0usize;
    let mut start = 0usize;
    let mut gap_score = 0usize;
    for needle in query.chars() {
        let relative = command[start..]
            .iter()
            .position(|candidate| *candidate == needle)?;
        let index = start + relative;
        gap_score = gap_score.saturating_add(index.saturating_sub(last_match));
        last_match = index;
        start = index.saturating_add(1);
    }
    Some(gap_score.min(u16::MAX as usize) as u16)
}

pub(crate) fn parse_slash_command(line: &str) -> Result<Option<SlashCommand>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('/') {
        return Ok(None);
    }
    let mut parts = trimmed.split_whitespace();
    let command = parts.next().unwrap_or_default();
    let rest = parts.collect::<Vec<_>>();
    let parsed = if let Some(name) = command.strip_prefix("/skill:") {
        if name.trim().is_empty() {
            return Err(anyhow!("usage: /skill:<name> [args]"));
        }
        SlashCommand::SkillInvoke {
            name: name.to_string(),
            args: rest.join(" "),
        }
    } else {
        let Some(spec) = slash_command_spec(command) else {
            return Err(anyhow!("unknown slash command: {command}"));
        };
        parse_registered_slash_command(spec, command, &rest)?
    };
    Ok(Some(parsed))
}

fn parse_registered_slash_command(
    spec: &SlashCommandSpec,
    command: &str,
    rest: &[&str],
) -> Result<SlashCommand> {
    debug_assert_eq!(spec.surface, CommandSurface::TuiSlash);
    if spec.status == CommandStatus::Upcoming {
        if !rest.is_empty() {
            return Err(anyhow!(
                "{command} is upcoming and does not accept arguments"
            ));
        }
        return Ok(SlashCommand::Upcoming(
            spec.canonical.trim_start_matches('/').to_string(),
        ));
    }

    match spec.action {
        SlashCommandAction::Help => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Help)
        }
        SlashCommandAction::Quit => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Quit)
        }
        SlashCommandAction::Status => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Status)
        }
        SlashCommandAction::New => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::New)
        }
        SlashCommandAction::Sessions => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Sessions)
        }
        SlashCommandAction::Usage => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Usage)
        }
        SlashCommandAction::Context => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Context)
        }
        SlashCommandAction::ModelShow => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::ModelShow)
        }
        SlashCommandAction::VariantSet => parse_variant_command(spec, rest),
        SlashCommandAction::ModeSet => parse_mode_command(spec, rest),
        SlashCommandAction::Thinking => parse_thinking_command(spec, rest),
        SlashCommandAction::Raw => parse_raw_command(spec, rest),
        SlashCommandAction::Copy => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Copy)
        }
        SlashCommandAction::Image => parse_image_command(spec, rest),
        SlashCommandAction::Rename => parse_rename_command(spec, rest),
        SlashCommandAction::Undo => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Undo)
        }
        SlashCommandAction::Redo => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Redo)
        }
        SlashCommandAction::Skills => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Skills)
        }
        SlashCommandAction::SkillInvoke => {
            unreachable!("dynamic skill commands are parsed before registry dispatch")
        }
        SlashCommandAction::Upcoming => unreachable!("upcoming handled before action dispatch"),
    }
}

fn parse_no_arguments(spec: &SlashCommandSpec, command: &str, rest: &[&str]) -> Result<()> {
    debug_assert_eq!(spec.argument_kind, CommandArgumentKind::None);
    if !rest.is_empty() {
        return Err(anyhow!("{command} does not accept arguments"));
    }
    Ok(())
}

fn parse_variant_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Err(anyhow!("usage: {}", spec.usage)),
        [value] => {
            validate_variant(value)?;
            Ok(SlashCommand::VariantSet((*value).to_string()))
        }
        _ => Err(anyhow!("usage: {}", spec.usage)),
    }
}

fn parse_mode_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Err(anyhow!("usage: {}", spec.usage)),
        [value] => {
            validate_mode(value)?;
            Ok(SlashCommand::ModeSet((*value).to_string()))
        }
        _ => Err(anyhow!("usage: {}", spec.usage)),
    }
}

fn parse_thinking_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Ok(SlashCommand::ThinkingToggle),
        ["on"] => Ok(SlashCommand::ThinkingSet(true)),
        ["off"] => Ok(SlashCommand::ThinkingSet(false)),
        _ => Err(anyhow!("usage: {}", spec.usage)),
    }
}

fn parse_raw_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Ok(SlashCommand::RawToggle),
        ["on"] => Ok(SlashCommand::RawSet(true)),
        ["off"] => Ok(SlashCommand::RawSet(false)),
        _ => Err(anyhow!("usage: {}", spec.usage)),
    }
}

fn parse_image_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    let input = rest.join(" ");
    let Some(argument) = split_image_source_argument(&input) else {
        return Err(anyhow!("usage: {}", spec.usage));
    };
    Ok(SlashCommand::Image {
        source: argument.source,
        prompt: argument.remainder,
    })
}

fn parse_rename_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    let title = rest.join(" ");
    if title.trim().is_empty() {
        return Err(anyhow!("usage: {}", spec.usage));
    }
    Ok(SlashCommand::Rename(title))
}

pub(crate) fn validate_model_spec(value: &str) -> Result<()> {
    let Some((provider, model)) = value.split_once('/') else {
        return Err(anyhow!("model must use provider/model form"));
    };
    if provider.trim().is_empty() || model.trim().is_empty() {
        return Err(anyhow!("model must use provider/model form"));
    }
    Ok(())
}

pub(crate) fn validate_variant(value: &str) -> Result<()> {
    if VARIANTS.contains(&value) {
        Ok(())
    } else {
        Err(anyhow!("variant must be one of {}", VARIANTS.join(", ")))
    }
}

pub(crate) fn validate_mode(value: &str) -> Result<()> {
    match value {
        "plan" | "default" => Ok(()),
        _ => Err(anyhow!("mode must be one of plan, default")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_slash_commands() {
        assert_eq!(
            parse_slash_command("/help").unwrap(),
            Some(SlashCommand::Help)
        );
        assert!(parse_slash_command("/help now").is_err());
        assert_eq!(parse_slash_command("/q").unwrap(), Some(SlashCommand::Quit));
        assert_eq!(parse_slash_command("hello").unwrap(), None);
    }

    #[test]
    fn parses_session_and_model_commands() {
        assert_eq!(
            parse_slash_command("/sessions").unwrap(),
            Some(SlashCommand::Sessions)
        );
        assert_eq!(
            parse_slash_command("/resume").unwrap(),
            Some(SlashCommand::Sessions)
        );
        assert_eq!(
            parse_slash_command("/continue").unwrap(),
            Some(SlashCommand::Sessions)
        );
        assert_eq!(
            parse_slash_command("/stats").unwrap(),
            Some(SlashCommand::Usage)
        );
        assert_eq!(
            parse_slash_command("/usage").unwrap(),
            Some(SlashCommand::Usage)
        );
        assert_eq!(
            parse_slash_command("/context").unwrap(),
            Some(SlashCommand::Context)
        );
        assert!(
            parse_slash_command("/session list")
                .unwrap_err()
                .to_string()
                .contains("unknown slash command: /session")
        );
        assert!(parse_slash_command("/stats all").is_err());
        assert!(parse_slash_command("/context now").is_err());
        assert!(parse_slash_command("/session show abc").is_err());
        assert!(parse_slash_command("/session switch abc").is_err());
        assert_eq!(
            parse_slash_command("/model").unwrap(),
            Some(SlashCommand::ModelShow)
        );
        assert!(
            parse_slash_command("/model set mock/model")
                .unwrap_err()
                .to_string()
                .contains("/model does not accept arguments")
        );
        assert!(parse_slash_command("/model fetch").is_err());
        assert!(
            parse_slash_command("/models")
                .unwrap_err()
                .to_string()
                .contains("unknown slash command: /models")
        );
    }

    #[test]
    fn validates_variants() {
        assert_eq!(
            parse_slash_command("/variant high").unwrap(),
            Some(SlashCommand::VariantSet("high".to_string()))
        );
        assert_eq!(
            parse_slash_command("/variant none").unwrap(),
            Some(SlashCommand::VariantSet("none".to_string()))
        );
        assert!(parse_slash_command("/variant").is_err());
        assert!(
            parse_slash_command("/variant set high")
                .unwrap_err()
                .to_string()
                .contains("usage: /variant <none|minimal|low|medium|high|xhigh|max>")
        );
        assert!(parse_slash_command("/effort high").is_err());
        assert!(parse_slash_command("/variant turbo").is_err());
    }

    #[test]
    fn parses_mode_commands() {
        assert_eq!(
            parse_slash_command("/mode plan").unwrap(),
            Some(SlashCommand::ModeSet("plan".to_string()))
        );
        assert_eq!(
            parse_slash_command("/mode default").unwrap(),
            Some(SlashCommand::ModeSet("default".to_string()))
        );
        assert!(parse_slash_command("/mode").is_err());
        assert!(
            parse_slash_command("/mode set plan")
                .unwrap_err()
                .to_string()
                .contains("usage: /mode <plan|default>")
        );
        assert!(parse_slash_command("/mode build").is_err());
        assert!(parse_slash_command("/mode maybe").is_err());
    }

    #[test]
    fn parses_thinking_visibility_commands() {
        assert_eq!(
            parse_slash_command("/show-thinking").unwrap(),
            Some(SlashCommand::ThinkingToggle)
        );
        assert_eq!(
            parse_slash_command("/show-thinking on").unwrap(),
            Some(SlashCommand::ThinkingSet(true))
        );
        assert_eq!(
            parse_slash_command("/show-thinking off").unwrap(),
            Some(SlashCommand::ThinkingSet(false))
        );
        assert!(parse_slash_command("/show-thinking maybe").is_err());
        assert!(
            parse_slash_command("/thinking")
                .unwrap_err()
                .to_string()
                .contains("unknown slash command: /thinking")
        );
    }

    #[test]
    fn parses_raw_visibility_and_copy_commands() {
        assert_eq!(
            parse_slash_command("/show-raw").unwrap(),
            Some(SlashCommand::RawToggle)
        );
        assert_eq!(
            parse_slash_command("/show-raw on").unwrap(),
            Some(SlashCommand::RawSet(true))
        );
        assert_eq!(
            parse_slash_command("/show-raw off").unwrap(),
            Some(SlashCommand::RawSet(false))
        );
        assert_eq!(
            parse_slash_command("/copy").unwrap(),
            Some(SlashCommand::Copy)
        );
        assert_eq!(
            parse_slash_command("/image \"image one.png\" describe").unwrap(),
            Some(SlashCommand::Image {
                source: "image one.png".to_string(),
                prompt: "describe".to_string(),
            })
        );
        assert!(parse_slash_command("/show-raw maybe").is_err());
        assert!(parse_slash_command("/copy now").is_err());
        assert!(parse_slash_command("/image").is_err());
        assert!(
            parse_slash_command("/raw")
                .unwrap_err()
                .to_string()
                .contains("unknown slash command: /raw")
        );
    }

    #[test]
    fn parses_session_rename_command() {
        assert_eq!(
            parse_slash_command("/rename My session").unwrap(),
            Some(SlashCommand::Rename("My session".to_string()))
        );
        assert!(parse_slash_command("/rename").is_err());
    }

    #[test]
    fn parses_skills_commands() {
        assert_eq!(
            parse_slash_command("/skills").unwrap(),
            Some(SlashCommand::Skills)
        );
        assert!(parse_slash_command("/skills now").is_err());
        assert_eq!(
            parse_slash_command("/skill:reviewer extra context").unwrap(),
            Some(SlashCommand::SkillInvoke {
                name: "reviewer".to_string(),
                args: "extra context".to_string(),
            })
        );
        assert!(parse_slash_command("/skill:").is_err());
    }

    #[test]
    fn slash_menu_filters_and_marks_upcoming() {
        assert_eq!(slash_menu_items("/he")[0].command, "/help");
        assert_eq!(slash_menu_items("/usage")[0].command, "/usage");
        assert!(
            slash_menu_items("/stats")
                .iter()
                .all(|item| item.command != "/stats")
        );
        assert!(slash_prefix_menu_items("/stats").is_empty());
        assert!(slash_menu_items("/clear").is_empty());
        assert!(slash_menu_items("/resume").is_empty());
        assert_eq!(slash_menu_items("/session").len(), 1);
        assert_eq!(slash_menu_items("/session")[0].command, "/sessions");
        assert!(slash_menu_items("/session ").is_empty());
        assert_eq!(slash_menu_items("/model").len(), 1);
        assert_eq!(slash_menu_items("/model")[0].command, "/model");
        assert_eq!(
            slash_menu_items("/model")[0].description,
            "select/fetch model"
        );
        let mode = slash_menu_items("/mode");
        assert_eq!(mode[0].command, "/mode");
        assert_eq!(mode[0].description, "set <plan|default>");
        let variant = slash_menu_items("/var");
        assert_eq!(variant[0].command, "/variant");
        assert_eq!(variant[0].description, "set <value>");
        let undo = slash_menu_items("/un");
        assert_eq!(undo.len(), 1);
        assert_eq!(undo[0].command, "/undo");
        assert!(!undo[0].upcoming);
        let rename = slash_menu_items("/ren");
        assert_eq!(rename.len(), 1);
        assert_eq!(rename[0].command, "/rename");
        assert_eq!(rename[0].description, "<title> rename current session");
        let fuzzy_rename = slash_menu_items("/rn");
        assert_eq!(fuzzy_rename[0].command, "/rename");
        let fuzzy_model = slash_menu_items("/mdl");
        assert_eq!(fuzzy_model[0].command, "/model");
        assert!(slash_prefix_menu_items("/rn").is_empty());
        assert_eq!(
            parse_slash_command("/undo").unwrap(),
            Some(SlashCommand::Undo)
        );
        assert_eq!(
            parse_slash_command("/redo").unwrap(),
            Some(SlashCommand::Redo)
        );
        assert!(parse_slash_command("/undo now").is_err());
        assert!(parse_slash_command("/redo now").is_err());
        assert_eq!(slash_menu_items("/copy")[0].command, "/copy");
        assert_eq!(slash_menu_items("/image")[0].command, "/image");
        assert_eq!(slash_menu_items("/show-raw")[0].command, "/show-raw");
    }

    #[test]
    fn formats_slash_help_from_registry() {
        let help = format_slash_help(Some(2));
        assert!(help.contains("General\n"));
        assert!(help.contains("\nCommands\n"));
        assert!(help.contains("\nCustom commands\n"));
        assert!(help.contains("Ctrl+B - toggle sidebar"));
        assert!(help.contains("Ctrl+O - copy latest answer as Markdown"));
        assert!(help.contains("/copy - copy latest answer as Markdown"));
        assert!(help.contains("/usage - usage and cost summary (aliases: /stats)"));
        assert!(help.contains("/sessions - switch session (aliases: /resume, /continue)"));
        assert!(help.contains("/skill:<name> [args] - invoke a skill (2 available)"));
        assert!(!help.contains("pevo run"));

        let empty = format_slash_help(Some(0));
        assert!(empty.contains("No custom commands available"));
    }

    #[test]
    fn slash_menu_can_filter_dynamic_skill_entries() {
        let mut items = base_slash_menu_items();
        items.push(SlashMenuItem {
            command: "/skill:reviewer".to_string(),
            description: "Review code changes".to_string(),
            upcoming: false,
        });

        let matches = slash_menu_items_from("/skill:r", &items);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].command, "/skill:reviewer");
        assert_eq!(matches[0].description, "Review code changes");
    }
}
