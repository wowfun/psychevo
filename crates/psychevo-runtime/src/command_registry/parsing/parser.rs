#[allow(unused_imports)]
pub(crate) use super::*;

pub const CUSTOM_SKILL_COMMAND: SlashCommandSpec = SlashCommandSpec {
    canonical: "/<skill-or-bundle>",
    aliases: &[],
    usage: "/<skill-or-bundle> [args]",
    summary: "submit a skill or bundle",
    help_detail: Some(
        "Submits the slash line while using the equivalent explicit skill or bundle marker for runtime expansion.",
    ),
    surface: TUI_SLASH,
    group: CommandGroup::CustomCommands,
    argument_kind: CommandArgumentKind::DynamicSuffixOptionalText,
    output_kind: CommandOutputKind::PromptSubmission,
    status: CommandStatus::Active,
    action: SlashCommandAction::SkillInvoke,
    common: false,
};

pub const CLI_COMMANDS: &[CliCommandSpec] = &[
    CliCommandSpec {
        canonical: "init",
        aliases: &[],
        usage: "pevo init",
        summary: "initialize Psychevo home",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "run",
        aliases: &[],
        usage: "pevo run [message..]",
        summary: "run one coding-agent turn",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "tui",
        aliases: &[],
        usage: "pevo tui [message..]",
        summary: "open the interactive terminal UI",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::FreeFormTrailingText,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "serve",
        aliases: &[],
        usage: "pevo serve",
        summary: "run the headless Gateway API server",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::OptionalValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "gateway",
        aliases: &[],
        usage: "pevo gateway [open|start|status|stop|restart]",
        summary: "manage the local Gateway Web Shell",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::OptionalValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "acp",
        aliases: &[],
        usage: "pevo acp",
        summary: "run the ACP stdio server",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::None,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "profile",
        aliases: &[],
        usage: "pevo profile <command>",
        summary: "manage local profiles",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "agent",
        aliases: &[],
        usage: "pevo agent <command>",
        summary: "manage agents",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "skill",
        aliases: &[],
        usage: "pevo skill <command>",
        summary: "manage local skills",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "tool",
        aliases: &["tools"],
        usage: "pevo tool <command>",
        summary: "manage local toolsets",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "session",
        aliases: &[],
        usage: "pevo session <command>",
        summary: "manage local sessions",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "model",
        aliases: &[],
        usage: "pevo model <command>",
        summary: "inspect configured models",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "config",
        aliases: &[],
        usage: "pevo config <command>",
        summary: "inspect and update Psychevo config",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "auth",
        aliases: &[],
        usage: "pevo auth <command>",
        summary: "inspect and update provider credentials",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "stats",
        aliases: &[],
        usage: "pevo stats",
        summary: "show local usage and estimated cost",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::OptionalValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
    CliCommandSpec {
        canonical: "context",
        aliases: &[],
        usage: "pevo context --session <id|latest>",
        summary: "inspect context-window usage for a session",
        surface: PEVO_CLI,
        group: COMMANDS,
        argument_kind: CommandArgumentKind::RequiredValue,
        output_kind: CommandOutputKind::ProcessResult,
        status: CommandStatus::Active,
    },
];

pub fn slash_command_spec(name: &str) -> Option<&'static SlashCommandSpec> {
    let name = normalize_slash_command_name(name)?;
    SLASH_COMMANDS
        .iter()
        .find(|spec| spec.canonical == name || spec.aliases.contains(&name))
}

pub fn parse_slash_command_line(input: &str) -> SlashCommandParse {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return SlashCommandParse::NotSlash;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or_default();
    if command == "/" {
        return SlashCommandParse::Unknown {
            original: trimmed.to_string(),
            command: command.to_string(),
            args: String::new(),
        };
    }
    let args = parts.next().unwrap_or("").trim().to_string();
    match slash_command_spec(command) {
        Some(spec) => SlashCommandParse::Known(SlashCommandInvocation {
            original: trimmed.to_string(),
            command: command.to_string(),
            args,
            spec,
        }),
        None => SlashCommandParse::Unknown {
            original: trimmed.to_string(),
            command: command.to_string(),
            args,
        },
    }
}

pub fn supported_by_capabilities(
    spec: &SlashCommandSpec,
    capabilities: &[CommandCapability],
) -> bool {
    spec.required_capabilities()
        .iter()
        .all(|capability| capabilities.contains(capability))
}

pub fn available_slash_commands_for_surface(
    capabilities: &[CommandCapability],
    active_turn: bool,
    dynamic: &[DynamicSlashCommand],
    cap: usize,
) -> AvailableSlashCommands {
    let mut commands = SLASH_COMMANDS
        .iter()
        .filter(|spec| spec.status == CommandStatus::Active)
        .filter(|spec| supported_by_capabilities(spec, capabilities))
        .filter(|spec| !active_turn || spec.available_during_active_turn())
        .map(|spec| AvailableSlashCommand {
            name: spec.canonical.trim_start_matches('/').to_string(),
            usage: spec.usage.to_string(),
            summary: spec.summary.to_string(),
            aliases: spec
                .aliases
                .iter()
                .map(|alias| (*alias).to_string())
                .collect(),
            argument_kind: spec.argument_kind,
            action: spec.action,
            presentation: spec.presentation(),
        })
        .collect::<Vec<_>>();
    let core_count = commands.len();
    if commands.len() > cap {
        commands.truncate(cap);
        return AvailableSlashCommands {
            commands,
            hidden_dynamic: dynamic.len(),
        };
    }
    if commands.len() < cap {
        let remaining = cap - commands.len();
        commands.extend(
            dynamic
                .iter()
                .take(remaining)
                .map(|command| AvailableSlashCommand {
                    name: command.name.trim_start_matches('/').to_string(),
                    usage: format!("/{} [args]", command.name.trim_start_matches('/')),
                    summary: command.summary.clone(),
                    aliases: Vec::new(),
                    argument_kind: CommandArgumentKind::DynamicSuffixOptionalText,
                    action: SlashCommandAction::SkillInvoke,
                    presentation: command_presentation(SlashCommandAction::SkillInvoke),
                }),
        );
    }
    AvailableSlashCommands {
        hidden_dynamic: dynamic.len().saturating_sub(cap.saturating_sub(core_count)),
        commands,
    }
}

pub fn slash_invocation_effect(
    invocation: &SlashCommandInvocation,
    capabilities: &[CommandCapability],
    surface: SlashCommandSurface,
    active_turn: bool,
) -> std::result::Result<SlashCommandEffect, String> {
    let spec = invocation.spec;
    if !supported_by_capabilities(spec, capabilities) {
        return Ok(SlashCommandEffect::Unsupported(
            spec.unavailable_guidance(surface),
        ));
    }
    if active_turn && !spec.available_during_active_turn() {
        return Ok(SlashCommandEffect::Unsupported(format!(
            "{} is not available while a turn is running; wait, cancel, or use /queue <message>.",
            spec.canonical
        )));
    }
    if spec.status == CommandStatus::Upcoming {
        if invocation.args.trim().is_empty() {
            return Ok(SlashCommandEffect::Unsupported(format!(
                "{} is not available yet.",
                spec.canonical
            )));
        }
        return Err(format!(
            "{} is upcoming and does not accept arguments",
            spec.canonical
        ));
    }

    match spec.action {
        SlashCommandAction::Help
        | SlashCommandAction::Status
        | SlashCommandAction::Usage
        | SlashCommandAction::Context
        | SlashCommandAction::Diff
        | SlashCommandAction::Refresh => {
            no_args(spec, &invocation.args)?;
            if spec.action == SlashCommandAction::Diff {
                Ok(SlashCommandEffect::Diff)
            } else {
                Ok(SlashCommandEffect::LocalText)
            }
        }
        SlashCommandAction::New => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::NewSession)
        }
        SlashCommandAction::Sessions => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::SessionsList)
        }
        SlashCommandAction::Resume => Ok(SlashCommandEffect::ResumeSession {
            reference: optional_text(&invocation.args),
        }),
        SlashCommandAction::Steer => Ok(SlashCommandEffect::Steer(required_text(
            spec,
            &invocation.args,
        )?)),
        SlashCommandAction::Queue => Ok(SlashCommandEffect::Queue(required_text(
            spec,
            &invocation.args,
        )?)),
        SlashCommandAction::Pending => match invocation.args.as_str() {
            "cancel" => Ok(SlashCommandEffect::PendingCancel),
            _ => Err(format!("usage: {}", spec.usage)),
        },
        SlashCommandAction::ModelShow => parse_model_effect(spec, &invocation.args),
        SlashCommandAction::VariantSet => Ok(SlashCommandEffect::SetVariant(parse_single_value(
            spec,
            &invocation.args,
            validate_variant,
        )?)),
        SlashCommandAction::ModeSet => Ok(SlashCommandEffect::SetMode(parse_single_value(
            spec,
            &invocation.args,
            validate_mode,
        )?)),
        SlashCommandAction::Permissions => parse_permissions_effect(&invocation.args),
        SlashCommandAction::Sandbox => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::SandboxShow)
        }
        SlashCommandAction::Tools => parse_tools_effect(&invocation.args),
        SlashCommandAction::Rename => Ok(SlashCommandEffect::Rename(required_text(
            spec,
            &invocation.args,
        )?)),
        SlashCommandAction::Undo => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::Undo)
        }
        SlashCommandAction::Redo => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::Redo)
        }
        SlashCommandAction::Skills => Ok(SlashCommandEffect::Skills {
            args: optional_text(&invocation.args),
        }),
        SlashCommandAction::Bundles => Ok(SlashCommandEffect::Bundles {
            args: optional_text(&invocation.args),
        }),
        SlashCommandAction::Curator => Ok(SlashCommandEffect::Curator {
            args: optional_text(&invocation.args),
        }),
        SlashCommandAction::Agents => {
            no_args(spec, &invocation.args)?;
            Ok(SlashCommandEffect::Agents)
        }
        SlashCommandAction::Fork => Ok(SlashCommandEffect::Fork(fork_prompt_marker(
            &required_text(spec, &invocation.args)?,
        ))),
        SlashCommandAction::Compact => Ok(SlashCommandEffect::Compact {
            instructions: optional_text(&invocation.args),
        }),
        SlashCommandAction::Export => Ok(SlashCommandEffect::Export {
            args: optional_text(&invocation.args),
        }),
        SlashCommandAction::Share => Ok(SlashCommandEffect::Share {
            args: optional_text(&invocation.args),
        }),
        SlashCommandAction::Btw => Ok(SlashCommandEffect::Btw {
            prompt: optional_text(&invocation.args),
        }),
        SlashCommandAction::Quit
        | SlashCommandAction::Thinking
        | SlashCommandAction::Raw
        | SlashCommandAction::Copy
        | SlashCommandAction::Image => Ok(SlashCommandEffect::Unsupported(
            spec.unavailable_guidance(surface),
        )),
        SlashCommandAction::SkillInvoke => unreachable!("dynamic skills use dynamic effect"),
    }
}

pub fn dynamic_slash_command_effect(
    command: &str,
    args: &str,
    dynamic: &[DynamicSlashCommand],
) -> Option<SlashCommandEffect> {
    let command = normalize_dynamic_skill_name(command.trim_start_matches('/'));
    dynamic
        .iter()
        .find(|entry| normalize_dynamic_skill_name(&entry.name) == command)
        .map(|entry| {
            let mut prompt = entry.prompt.trim().to_string();
            if !args.trim().is_empty() {
                if !prompt.ends_with(' ') {
                    prompt.push(' ');
                }
                prompt.push_str(args.trim());
            }
            SlashCommandEffect::SubmitPrompt(prompt)
        })
}

pub fn skill_prompt_marker(name: &str, args: &str) -> String {
    if args.trim().is_empty() {
        format!("${} ", name.trim_start_matches('/'))
    } else {
        format!("${} {}", name.trim_start_matches('/'), args.trim())
    }
}

pub fn normalize_dynamic_skill_name(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch)
            } else if ch == '-' || ch == '_' || ch.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn fork_prompt_marker(prompt: &str) -> String {
    format!(
        "Use the spawn_agent tool with agent_type=\"general\", fork_context=true, background=true, a lowercase underscore task_name, and this message:\n\n{}",
        prompt.trim()
    )
}

pub(crate) fn normalize_slash_command_name(name: &str) -> Option<&str> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    name.starts_with('/').then_some(name)
}

pub(crate) fn no_args(spec: &SlashCommandSpec, args: &str) -> std::result::Result<(), String> {
    if args.trim().is_empty() {
        Ok(())
    } else {
        Err(format!("{} does not accept arguments", spec.canonical))
    }
}

pub(crate) fn required_text(
    spec: &SlashCommandSpec,
    args: &str,
) -> std::result::Result<String, String> {
    let args = args.trim();
    if args.is_empty() {
        Err(format!("usage: {}", spec.usage))
    } else {
        Ok(args.to_string())
    }
}

pub(crate) fn optional_text(args: &str) -> Option<String> {
    let args = args.trim();
    (!args.is_empty()).then(|| args.to_string())
}

pub(crate) fn parse_single_value(
    spec: &SlashCommandSpec,
    args: &str,
    validate: fn(&str) -> bool,
) -> std::result::Result<String, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [value] if validate(value) => Ok((*value).to_string()),
        _ => Err(format!("usage: {}", spec.usage)),
    }
}

pub(crate) fn parse_model_effect(
    spec: &SlashCommandSpec,
    args: &str,
) -> std::result::Result<SlashCommandEffect, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["--local"] | ["-g"] | ["--global"] => Ok(SlashCommandEffect::ShowModel),
        ["set", ..] => Err(format!("usage: {}", spec.usage)),
        [flag] if flag.starts_with('-') => Err(format!("usage: {}", spec.usage)),
        [model] => Ok(SlashCommandEffect::SetModel {
            model: (*model).to_string(),
            variant: None,
        }),
        [model, variant] if validate_variant(variant) => Ok(SlashCommandEffect::SetModel {
            model: (*model).to_string(),
            variant: Some((*variant).to_string()),
        }),
        _ => Err(format!("usage: {}", spec.usage)),
    }
}

pub(crate) fn parse_tools_effect(args: &str) -> std::result::Result<SlashCommandEffect, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["list"] => Ok(SlashCommandEffect::ToolsShow),
        ["enable", name] => Ok(SlashCommandEffect::ToolsetSet {
            name: (*name).to_string(),
            enabled: true,
        }),
        ["disable", name] => Ok(SlashCommandEffect::ToolsetSet {
            name: (*name).to_string(),
            enabled: false,
        }),
        _ => Err("usage: /tools [list|enable|disable <toolset>]".to_string()),
    }
}

pub(crate) fn parse_permissions_effect(
    args: &str,
) -> std::result::Result<SlashCommandEffect, String> {
    let args = args.trim();
    if args.is_empty() {
        return Ok(SlashCommandEffect::PermissionsShow);
    }
    let mut parts = args.split_whitespace();
    let Some(action) = parts.next() else {
        return Ok(SlashCommandEffect::PermissionsShow);
    };
    match action {
        "allow" | "ask" | "deny" => {
            let rule = parts.collect::<Vec<_>>().join(" ");
            if rule.trim().is_empty() {
                return Err("usage: /permissions allow|ask|deny <rule>".to_string());
            }
            Ok(SlashCommandEffect::PermissionAdd {
                kind: action.to_string(),
                rule,
            })
        }
        "remove" => {
            let Some(kind) = parts.next() else {
                return Err("usage: /permissions remove <allow|ask|deny> <rule>".to_string());
            };
            if !matches!(kind, "allow" | "ask" | "deny") {
                return Err("usage: /permissions remove <allow|ask|deny> <rule>".to_string());
            }
            let rule = parts.collect::<Vec<_>>().join(" ");
            if rule.trim().is_empty() {
                return Err("usage: /permissions remove <allow|ask|deny> <rule>".to_string());
            }
            Ok(SlashCommandEffect::PermissionRemove {
                kind: kind.to_string(),
                rule,
            })
        }
        _ => Err(
            "usage: /permissions [allow|ask|deny <rule>|remove <allow|ask|deny> <rule>]"
                .to_string(),
        ),
    }
}

pub(crate) fn validate_variant(value: &str) -> bool {
    matches!(
        value,
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
    )
}

pub(crate) fn validate_mode(value: &str) -> bool {
    matches!(value, "plan" | "default")
}
