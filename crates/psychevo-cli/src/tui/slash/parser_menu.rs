#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn normalize_key_code(code: &KeyCode) -> KeyCode {
    match code {
        KeyCode::Char(ch) => KeyCode::Char(ch.to_ascii_lowercase()),
        other => *other,
    }
}

pub(crate) fn normalized_modifiers(modifiers: KeyModifiers) -> KeyModifiers {
    modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT)
}

pub(crate) fn split_command_token(line: &str) -> (&str, &str) {
    let trimmed = line.trim();
    match trimmed.find(char::is_whitespace) {
        Some(index) => (&trimmed[..index], &trimmed[index..]),
        None => (trimmed, ""),
    }
}

pub(crate) fn canonical_command_token(line: &str) -> Option<String> {
    let (command, _) = split_command_token(line);
    slash_command_spec(command).map(|spec| spec.canonical.to_string())
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
pub(crate) enum MatchMode {
    Prefix,
    Fuzzy,
}

pub(crate) fn slash_menu_items_for(
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
            best_slash_item_match(item, &query, mode).map(|matched| (matched, index, item))
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|(matched, index, _)| {
        (
            matched.score,
            usize::from(!matched.configured_alias),
            *index,
            matched.alias_index,
        )
    });
    let mut items = items
        .into_iter()
        .map(|(matched, _, item)| {
            let mut item = item.clone();
            item.command = matched.command;
            item.replacement = matched.replacement;
            item.completion = matched.completion;
            item.configured_alias = matched.configured_alias;
            if let Some(description) = matched.description {
                item.description = description;
            }
            item
        })
        .take(8)
        .collect::<Vec<_>>();
    if query != "/"
        && let Some(index) = items.iter().position(|item| item.command == query)
    {
        let exact = items.remove(index);
        items.insert(0, exact);
    }
    items
}

pub(crate) struct SlashItemMatch {
    pub(crate) score: u16,
    pub(crate) alias_index: usize,
    pub(crate) replacement: String,
    pub(crate) completion: String,
    pub(crate) command: String,
    pub(crate) description: Option<String>,
    pub(crate) configured_alias: bool,
}

pub(crate) fn best_slash_item_match(
    item: &SlashMenuItem,
    query: &str,
    mode: MatchMode,
) -> Option<SlashItemMatch> {
    let canonical = slash_match_score(&item.command, query, mode).map(|score| SlashItemMatch {
        score,
        alias_index: 0,
        replacement: item.command.clone(),
        completion: item.command.clone(),
        command: item.command.clone(),
        description: None,
        configured_alias: false,
    });
    let alias = item
        .aliases
        .iter()
        .enumerate()
        .filter_map(|(index, alias)| {
            slash_match_score(&alias.alias, query, mode).map(|score| SlashItemMatch {
                score,
                alias_index: index.saturating_add(1),
                replacement: alias.replacement.clone(),
                completion: alias.alias.clone(),
                command: if alias.target.is_some() {
                    alias.alias.clone()
                } else {
                    item.command.clone()
                },
                description: alias
                    .target
                    .as_ref()
                    .map(|target| format!("alias for {target} - {}", item.description)),
                configured_alias: alias.target.is_some(),
            })
        });
    canonical.into_iter().chain(alias).min_by_key(|matched| {
        let configured_alias_priority = usize::from(!matched.configured_alias);
        (
            matched.score,
            configured_alias_priority,
            matched.alias_index,
        )
    })
}

pub(crate) fn slash_match_score(command: &str, query: &str, mode: MatchMode) -> Option<u16> {
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

pub(crate) fn fuzzy_subsequence_score(command: &str, query: &str) -> Option<u16> {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TuiSlashParse {
    NotSlash,
    Command(SlashCommand),
    Unknown {
        original: String,
        command: String,
        args: String,
    },
}

#[cfg(test)]
pub(crate) fn parse_slash_command(line: &str) -> Result<Option<SlashCommand>> {
    parse_slash_command_inner(line)
}

pub(crate) fn parse_slash_command_with_config(
    line: &str,
    config: &EffectiveSlashConfig,
) -> Result<Option<SlashCommand>> {
    if let Some(expanded) = config.expand_alias_line(line) {
        return parse_slash_command_inner(&expanded);
    }
    parse_slash_command_inner(line)
}

pub(crate) fn parse_slash_command_inner(line: &str) -> Result<Option<SlashCommand>> {
    match parse_tui_slash_inner(line, line.trim().to_string())? {
        TuiSlashParse::NotSlash => Ok(None),
        TuiSlashParse::Command(command) => Ok(Some(command)),
        TuiSlashParse::Unknown { command, .. } => Err(anyhow!("unknown slash command: {command}")),
    }
}

pub(crate) fn parse_tui_slash_with_config(
    line: &str,
    config: &EffectiveSlashConfig,
) -> Result<TuiSlashParse> {
    if let Some(expanded) = config.expand_alias_line(line) {
        return parse_tui_slash_inner(&expanded, line.to_string());
    }
    parse_tui_slash_inner(line, line.to_string())
}

pub(crate) fn parse_tui_slash_inner(line: &str, original: String) -> Result<TuiSlashParse> {
    let trimmed = line.trim();
    if !trimmed.starts_with('/') {
        return Ok(TuiSlashParse::NotSlash);
    }
    let mut parts = trimmed.split_whitespace();
    let command = parts.next().unwrap_or_default();
    let rest = parts.collect::<Vec<_>>();
    let parsed = if command.starts_with(OLD_DYNAMIC_SKILL_PREFIX) {
        return Err(anyhow!("usage: /<skill-or-bundle> [args]"));
    } else if command == "/side" {
        SlashCommand::Btw(parse_btw_prompt(&rest))
    } else if command == "/reload-context" {
        parse_no_arguments_for_usage(command, &rest)?;
        SlashCommand::ReloadContextDeprecated
    } else {
        let Some(spec) = slash_command_spec(command) else {
            if OBSOLETE_SLASH_COMMAND_TOKENS.contains(&command) {
                return Ok(TuiSlashParse::Unknown {
                    original,
                    command: command.to_string(),
                    args: rest.join(" "),
                });
            }
            if let Some(name) = dynamic_skill_name(command) {
                return Ok(TuiSlashParse::Command(SlashCommand::SkillInvoke {
                    name,
                    args: rest.join(" "),
                }));
            }
            return Ok(TuiSlashParse::Unknown {
                original,
                command: command.to_string(),
                args: rest.join(" "),
            });
        };
        parse_registered_slash_command(spec, command, &rest)?
    };
    Ok(TuiSlashParse::Command(parsed))
}

pub(crate) fn dynamic_skill_name(command: &str) -> Option<String> {
    let name = command.strip_prefix('/')?;
    if name.is_empty()
        || name.starts_with('-')
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return None;
    }
    Some(name.to_string())
}

pub(crate) fn parse_registered_slash_command(
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
        SlashCommandAction::Resume => {
            parse_no_arguments_for_usage(command, rest)?;
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
        SlashCommandAction::Diff => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Diff)
        }
        SlashCommandAction::Refresh => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Refresh)
        }
        SlashCommandAction::Btw => Ok(SlashCommand::Btw(parse_btw_prompt(rest))),
        SlashCommandAction::Steer => parse_required_trailing(spec, rest).map(SlashCommand::Steer),
        SlashCommandAction::Queue => parse_required_trailing(spec, rest).map(SlashCommand::Queue),
        SlashCommandAction::Pending => parse_pending_command(spec, rest),
        SlashCommandAction::ModelShow => parse_model_command(command, rest),
        SlashCommandAction::VariantSet => parse_variant_command(spec, rest),
        SlashCommandAction::ModeSet => parse_mode_command(spec, rest),
        SlashCommandAction::Permissions => {
            parse_no_arguments_for_usage(command, rest)?;
            Ok(SlashCommand::Permissions)
        }
        SlashCommandAction::Sandbox => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Sandbox)
        }
        SlashCommandAction::Thinking => parse_thinking_command(spec, rest),
        SlashCommandAction::Raw => parse_raw_command(spec, rest),
        SlashCommandAction::Copy => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Copy)
        }
        SlashCommandAction::Export => parse_export_command(spec, rest),
        SlashCommandAction::Share => parse_share_command(spec, rest),
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
        SlashCommandAction::Skills => Ok(SlashCommand::Skills(parse_optional_trailing(rest))),
        SlashCommandAction::Tools => {
            parse_no_arguments_for_usage(command, rest)?;
            Ok(SlashCommand::Tools)
        }
        SlashCommandAction::Bundles => Ok(SlashCommand::Bundles(parse_optional_trailing(rest))),
        SlashCommandAction::Curator => Ok(SlashCommand::Curator(parse_optional_trailing(rest))),
        SlashCommandAction::Agents => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Agents)
        }
        SlashCommandAction::Fork => parse_fork_command(spec, rest),
        SlashCommandAction::Mission => parse_mission_command(spec, rest),
        SlashCommandAction::Compact => Ok(SlashCommand::Compact(parse_optional_trailing(rest))),
        SlashCommandAction::Voice => Err(anyhow!(
            "/voice is available in Workbench and messaging channels."
        )),
        SlashCommandAction::SkillInvoke => {
            unreachable!("dynamic skill commands are parsed before registry dispatch")
        }
    }
}

pub(crate) fn parse_no_arguments(
    spec: &SlashCommandSpec,
    command: &str,
    rest: &[&str],
) -> Result<()> {
    debug_assert_eq!(spec.argument_kind, CommandArgumentKind::None);
    parse_no_arguments_for_usage(command, rest)
}

pub(crate) fn parse_no_arguments_for_usage(command: &str, rest: &[&str]) -> Result<()> {
    if !rest.is_empty() {
        return Err(anyhow!("{command} does not accept arguments"));
    }
    Ok(())
}

pub(crate) fn parse_variant_command(
    spec: &SlashCommandSpec,
    rest: &[&str],
) -> Result<SlashCommand> {
    match rest {
        [] => Err(anyhow!("usage: {}", spec.usage)),
        [value] => {
            validate_variant(value)?;
            Ok(SlashCommand::VariantSet((*value).to_string()))
        }
        _ => Err(anyhow!("usage: {}", spec.usage)),
    }
}

pub(crate) fn parse_mode_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Err(anyhow!("usage: {}", spec.usage)),
        [value] => {
            validate_mode(value)?;
            Ok(SlashCommand::ModeSet((*value).to_string()))
        }
        _ => Err(anyhow!("usage: {}", spec.usage)),
    }
}

pub(crate) fn parse_thinking_command(
    spec: &SlashCommandSpec,
    rest: &[&str],
) -> Result<SlashCommand> {
    match rest {
        [] => Ok(SlashCommand::ThinkingToggle),
        ["on"] => Ok(SlashCommand::ThinkingSet(true)),
        ["off"] => Ok(SlashCommand::ThinkingSet(false)),
        _ => Err(anyhow!("usage: {}", spec.usage)),
    }
}

pub(crate) fn parse_raw_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] => Ok(SlashCommand::RawToggle),
        ["on"] => Ok(SlashCommand::RawSet(true)),
        ["off"] => Ok(SlashCommand::RawSet(false)),
        _ => Err(anyhow!("usage: {}", spec.usage)),
    }
}

pub(crate) fn parse_required_trailing(spec: &SlashCommandSpec, rest: &[&str]) -> Result<String> {
    let text = rest.join(" ");
    let text = text.trim();
    if text.is_empty() {
        return Err(anyhow!("usage: {}", spec.usage));
    }
    Ok(text.to_string())
}

pub(crate) fn parse_pending_command(
    spec: &SlashCommandSpec,
    rest: &[&str],
) -> Result<SlashCommand> {
    match rest {
        ["cancel"] => Ok(SlashCommand::PendingCancel),
        _ => Err(anyhow!("usage: {}", spec.usage)),
    }
}

pub(crate) fn parse_export_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    let parsed = parse_session_export_command_args(
        &rest.join(" "),
        SessionArtifactKind::Export,
        spec.usage,
    )?;
    Ok(SlashCommand::Export(TuiExportOptions {
        path: parsed.path,
        format: parsed.format,
        include: parsed.include,
    }))
}

pub(crate) fn parse_share_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    let parsed =
        parse_session_export_command_args(&rest.join(" "), SessionArtifactKind::Share, spec.usage)?;
    Ok(SlashCommand::Share(TuiShareOptions {
        path: parsed.path,
        include: parsed.include,
    }))
}

pub(crate) fn parse_image_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    let input = rest.join(" ");
    let Some(argument) = split_image_source_argument(&input) else {
        return Err(anyhow!("usage: {}", spec.usage));
    };
    Ok(SlashCommand::Image {
        source: argument.source,
        prompt: argument.remainder,
    })
}

pub(crate) fn parse_rename_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    let title = rest.join(" ");
    if title.trim().is_empty() {
        return Err(anyhow!("usage: {}", spec.usage));
    }
    Ok(SlashCommand::Rename(title))
}

pub(crate) fn parse_fork_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    let prompt = rest.join(" ");
    if prompt.trim().is_empty() {
        return Err(anyhow!("usage: {}", spec.usage));
    }
    Ok(SlashCommand::Fork(prompt))
}

pub(crate) fn parse_mission_command(
    spec: &SlashCommandSpec,
    rest: &[&str],
) -> Result<SlashCommand> {
    let args = rest.join(" ");
    if args.trim().is_empty() {
        return Err(anyhow!("usage: {}", spec.usage));
    }
    let (team, goal) = parse_mission_args(&args).map_err(|message| anyhow!(message))?;
    Ok(SlashCommand::Mission { team, goal })
}

pub(crate) fn parse_optional_trailing(rest: &[&str]) -> Option<String> {
    let text = rest.join(" ");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

pub(crate) fn parse_btw_prompt(rest: &[&str]) -> Option<String> {
    let prompt = rest.join(" ");
    let prompt = prompt.trim();
    (!prompt.is_empty()).then(|| prompt.to_string())
}

pub(crate) fn parse_model_command(command: &str, rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        [] | ["--local"] => Ok(SlashCommand::ModelShowScoped { global: false }),
        ["-g"] | ["--global"] => Ok(SlashCommand::ModelShowScoped { global: true }),
        _ => Err(anyhow!("usage: {command} [--local|-g|--global]")),
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
