use anyhow::{Result, anyhow};

pub(crate) const VARIANTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlashCommand {
    Quit,
    Status,
    New,
    Sessions,
    ModelShow,
    VariantSet(String),
    ModeSet(String),
    ThinkingToggle,
    ThinkingSet(bool),
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

const SLASH_MENU: &[(&str, &str, bool)] = &[
    ("/status", "show local status", false),
    ("/new", "start a new session on next prompt", false),
    ("/sessions", "switch session", false),
    ("/model", "select/fetch model", false),
    ("/variant", "set <value>", false),
    ("/mode", "set <plan|default>", false),
    ("/show-thinking", "toggle; set <on|off>", false),
    ("/rename", "<title> rename current session", false),
    ("/undo", "undo last message", false),
    ("/redo", "redo undone messages", false),
    ("/skills", "list skills", false),
    ("/compact", "upcoming", true),
    ("/export", "upcoming", true),
    ("/quit", "quit TUI", false),
];

pub(crate) fn base_slash_menu_items() -> Vec<SlashMenuItem> {
    SLASH_MENU
        .iter()
        .map(|(command, description, upcoming)| SlashMenuItem {
            command: (*command).to_string(),
            description: (*description).to_string(),
            upcoming: *upcoming,
        })
        .collect()
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
        match command {
            "/quit" | "/exit" | "/q" => SlashCommand::Quit,
            "/status" => SlashCommand::Status,
            "/clear" | "/new" => SlashCommand::New,
            "/sessions" | "/resume" | "/continue" => {
                if !rest.is_empty() {
                    return Err(anyhow!("{command} does not accept arguments"));
                }
                SlashCommand::Sessions
            }
            "/session" => {
                return Err(anyhow!("usage: /sessions, /resume, or /continue"));
            }
            "/model" => parse_model_command(&rest)?,
            "/models" => return Err(anyhow!("/models has been removed; use /model")),
            "/variant" => parse_variant_command(&rest)?,
            "/mode" => parse_mode_command(&rest)?,
            "/show-thinking" => parse_thinking_command(&rest)?,
            "/thinking" => return Err(anyhow!("/thinking has been removed; use /show-thinking")),
            "/rename" => parse_rename_command(&rest)?,
            "/undo" => {
                if !rest.is_empty() {
                    return Err(anyhow!("/undo does not accept arguments"));
                }
                SlashCommand::Undo
            }
            "/redo" => {
                if !rest.is_empty() {
                    return Err(anyhow!("/redo does not accept arguments"));
                }
                SlashCommand::Redo
            }
            "/skills" => {
                if !rest.is_empty() {
                    return Err(anyhow!("/skills does not accept arguments"));
                }
                SlashCommand::Skills
            }
            "/compact" | "/export" => {
                if !rest.is_empty() {
                    return Err(anyhow!(
                        "{command} is upcoming and does not accept arguments"
                    ));
                }
                SlashCommand::Upcoming(command.trim_start_matches('/').to_string())
            }
            _ => return Err(anyhow!("unknown slash command: {command}")),
        }
    };
    Ok(Some(parsed))
}

fn parse_model_command(rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Ok(SlashCommand::ModelShow),
        ["set", ..] => Err(anyhow!("/model set has been removed; use /model")),
        _ => Err(anyhow!("usage: /model")),
    }
}

fn parse_variant_command(rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Err(anyhow!("usage: /variant <value>")),
        ["set", ..] => Err(anyhow!(
            "/variant set has been removed; use /variant <value>"
        )),
        [value] => {
            validate_variant(value)?;
            Ok(SlashCommand::VariantSet((*value).to_string()))
        }
        _ => Err(anyhow!("usage: /variant <value>")),
    }
}

fn parse_mode_command(rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Err(anyhow!("usage: /mode <plan|default>")),
        ["set", ..] => Err(anyhow!("/mode set has been removed; use /mode <value>")),
        [value] => {
            validate_mode(value)?;
            Ok(SlashCommand::ModeSet((*value).to_string()))
        }
        _ => Err(anyhow!("usage: /mode <plan|default>")),
    }
}

fn parse_thinking_command(rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Ok(SlashCommand::ThinkingToggle),
        ["on"] => Ok(SlashCommand::ThinkingSet(true)),
        ["off"] => Ok(SlashCommand::ThinkingSet(false)),
        _ => Err(anyhow!("usage: /show-thinking [on|off]")),
    }
}

fn parse_rename_command(rest: &[&str]) -> Result<SlashCommand> {
    let title = rest.join(" ");
    if title.trim().is_empty() {
        return Err(anyhow!("usage: /rename <title>"));
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
        assert!(parse_slash_command("/help").is_err());
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
        assert!(parse_slash_command("/session list").is_err());
        assert!(parse_slash_command("/session show abc").is_err());
        assert!(parse_slash_command("/session switch abc").is_err());
        assert_eq!(
            parse_slash_command("/model").unwrap(),
            Some(SlashCommand::ModelShow)
        );
        assert!(parse_slash_command("/model set mock/model").is_err());
        assert!(parse_slash_command("/model fetch").is_err());
        assert!(parse_slash_command("/models").is_err());
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
                .contains("use /variant <value>")
        );
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
                .contains("use /mode <value>")
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
        assert!(parse_slash_command("/thinking").is_err());
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
        assert!(slash_menu_items("/he").is_empty());
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
