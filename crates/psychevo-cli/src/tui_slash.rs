use anyhow::{Result, anyhow};

pub(crate) const VARIANTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlashCommand {
    Help,
    Quit,
    Status,
    New,
    SessionList,
    SessionShow(Option<String>),
    SessionSwitch(String),
    ModelShow,
    Models,
    ModelSet(String),
    VariantShow,
    VariantSet(String),
    ModeShow,
    ModeSet(String),
    ThinkingToggle,
    ThinkingSet(bool),
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
        command: "/help",
        description: "show commands",
        upcoming: false,
    },
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
        command: "/session list",
        description: "list workdir sessions",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/session show",
        description: "show sanitized session transcript",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/model",
        description: "show current model",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/models",
        description: "list configured models",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/model set",
        description: "persist workdir model",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/variant",
        description: "show current variant",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/mode",
        description: "show current mode",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/thinking",
        description: "toggle thinking visibility",
        upcoming: false,
    },
    SlashMenuItem {
        command: "/undo",
        description: "upcoming",
        upcoming: true,
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
    SLASH_MENU
        .iter()
        .filter(|item| item.command.starts_with(&prefix) || prefix == "/")
        .take(8)
        .cloned()
        .collect()
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
        "/help" => SlashCommand::Help,
        "/quit" | "/exit" | "/q" => SlashCommand::Quit,
        "/status" => SlashCommand::Status,
        "/clear" | "/new" => SlashCommand::New,
        "/session" => parse_session_command(&rest)?,
        "/model" => parse_model_command(&rest)?,
        "/models" => {
            if !rest.is_empty() {
                return Err(anyhow!("/models does not accept arguments"));
            }
            SlashCommand::Models
        }
        "/variant" => parse_variant_command(&rest)?,
        "/mode" => parse_mode_command(&rest)?,
        "/thinking" => parse_thinking_command(&rest)?,
        "/undo" | "/compact" | "/export" => {
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

fn parse_session_command(rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        ["list"] => Ok(SlashCommand::SessionList),
        ["show"] => Ok(SlashCommand::SessionShow(None)),
        ["show", id] => Ok(SlashCommand::SessionShow(Some((*id).to_string()))),
        ["switch", id] => Ok(SlashCommand::SessionSwitch((*id).to_string())),
        [] => Ok(SlashCommand::SessionList),
        _ => Err(anyhow!(
            "usage: /session list | /session show [id] | /session switch <id|prefix|latest>"
        )),
    }
}

fn parse_model_command(rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Ok(SlashCommand::ModelShow),
        ["set", model] => {
            validate_model_spec(model)?;
            Ok(SlashCommand::ModelSet((*model).to_string()))
        }
        _ => Err(anyhow!(
            "usage: /model | /models | /model set <provider/model>"
        )),
    }
}

fn parse_variant_command(rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Ok(SlashCommand::VariantShow),
        ["set", value] => {
            validate_variant(value)?;
            Ok(SlashCommand::VariantSet((*value).to_string()))
        }
        _ => Err(anyhow!("usage: /variant | /variant set <value>")),
    }
}

fn parse_mode_command(rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Ok(SlashCommand::ModeShow),
        ["set", value] => {
            validate_mode(value)?;
            Ok(SlashCommand::ModeSet((*value).to_string()))
        }
        _ => Err(anyhow!("usage: /mode | /mode set <plan|default>")),
    }
}

fn parse_thinking_command(rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Ok(SlashCommand::ThinkingToggle),
        ["on"] => Ok(SlashCommand::ThinkingSet(true)),
        ["off"] => Ok(SlashCommand::ThinkingSet(false)),
        _ => Err(anyhow!("usage: /thinking [on|off]")),
    }
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
        assert_eq!(parse_slash_command("/q").unwrap(), Some(SlashCommand::Quit));
        assert_eq!(parse_slash_command("hello").unwrap(), None);
    }

    #[test]
    fn parses_session_and_model_commands() {
        assert_eq!(
            parse_slash_command("/session switch abc").unwrap(),
            Some(SlashCommand::SessionSwitch("abc".to_string()))
        );
        assert_eq!(
            parse_slash_command("/model set mock/model").unwrap(),
            Some(SlashCommand::ModelSet("mock/model".to_string()))
        );
        assert!(parse_slash_command("/model set model").is_err());
    }

    #[test]
    fn validates_variants() {
        assert!(parse_slash_command("/variant set high").is_ok());
        assert!(parse_slash_command("/variant set turbo").is_err());
    }

    #[test]
    fn parses_mode_commands() {
        assert_eq!(
            parse_slash_command("/mode").unwrap(),
            Some(SlashCommand::ModeShow)
        );
        assert_eq!(
            parse_slash_command("/mode set plan").unwrap(),
            Some(SlashCommand::ModeSet("plan".to_string()))
        );
        assert_eq!(
            parse_slash_command("/mode set default").unwrap(),
            Some(SlashCommand::ModeSet("default".to_string()))
        );
        assert!(parse_slash_command("/mode set build").is_err());
        assert!(parse_slash_command("/mode set maybe").is_err());
    }

    #[test]
    fn parses_thinking_visibility_commands() {
        assert_eq!(
            parse_slash_command("/thinking").unwrap(),
            Some(SlashCommand::ThinkingToggle)
        );
        assert_eq!(
            parse_slash_command("/thinking on").unwrap(),
            Some(SlashCommand::ThinkingSet(true))
        );
        assert_eq!(
            parse_slash_command("/thinking off").unwrap(),
            Some(SlashCommand::ThinkingSet(false))
        );
        assert!(parse_slash_command("/thinking maybe").is_err());
    }

    #[test]
    fn slash_menu_filters_and_marks_upcoming() {
        let undo = slash_menu_items("/un");
        assert_eq!(undo.len(), 1);
        assert_eq!(undo[0].command, "/undo");
        assert!(undo[0].upcoming);
        assert_eq!(
            parse_slash_command("/undo").unwrap(),
            Some(SlashCommand::Upcoming("undo".to_string()))
        );
    }
}
