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
    Upcoming(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlashMenuItem {
    pub(crate) command: &'static str,
    pub(crate) description: &'static str,
    pub(crate) upcoming: bool,
}

const SLASH_MENU: &[SlashMenuItem] = &[
    SlashMenuItem {
        command: "/status",
        description: "show local status",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/new",
        description: "start a new session on next prompt",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/sessions",
        description: "switch session",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/model",
        description: "select/fetch model",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/variant",
        description: "set <value>",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/mode",
        description: "set <plan|default>",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/show-thinking",
        description: "toggle; set <on|off>",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/rename",
        description: "<title> rename current session",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/undo",
        description: "undo last message",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/redo",
        description: "redo undone messages",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/compact",
        description: "upcoming",
        upcoming: true,
    },
    SlashMenuItem {
        command: "/export",
        description: "upcoming",
        upcoming: true,
    },
    SlashMenuItem {
        command: "/quit",
        description: "quit TUI",
        upcoming: false,
    },
];

pub(crate) fn slash_menu_items(input: &str) -> Vec<SlashMenuItem> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') || trimmed.contains('\n') {
        return Vec::new();
    }
    let prefix = trimmed.to_lowercase();
    let mut items = SLASH_MENU
        .iter()
        .filter(|item| item.command.starts_with(&prefix) || prefix == "/")
        .take(8)
        .cloned()
        .collect::<Vec<_>>();
    if prefix != "/"
        && let Some(index) = items.iter().position(|item| item.command == prefix)
    {
        let exact = items.remove(index);
        items.insert(0, exact);
    }
    items
}

pub(crate) fn parse_slash_command(line: &str) -> Result<Option<SlashCommand>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('/') {
        return Ok(None);
    }
    let mut parts = trimmed.split_whitespace();
    let command = parts.next().unwrap_or_default();
    let rest = parts.collect::<Vec<_>>();
    let parsed = match command {
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
        "/compact" | "/export" => {
            if !rest.is_empty() {
                return Err(anyhow!(
                    "{command} is upcoming and does not accept arguments"
                ));
            }
            SlashCommand::Upcoming(command.trim_start_matches('/').to_string())
        }
        _ => return Err(anyhow!("unknown slash command: {command}")),
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
}
