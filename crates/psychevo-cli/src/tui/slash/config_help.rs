#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) const VARIANTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];
pub(crate) const GENERAL_COMMANDS: &[&str] = &[
    "/status",
    "/context",
    "/diff",
    "/refresh",
    "/btw",
    "/model",
    "/sandbox",
    "/tools",
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
    Diff,
    Refresh,
    ReloadContextDeprecated,
    Btw(Option<String>),
    Steer(String),
    Queue(String),
    PendingCancel,
    ModelShowScoped { global: bool },
    VariantSet(String),
    ModeSet(String),
    Permissions,
    Sandbox,
    ThinkingToggle,
    ThinkingSet(bool),
    RawToggle,
    RawSet(bool),
    Copy,
    Export(TuiExportOptions),
    Share(TuiShareOptions),
    Image { source: String, prompt: String },
    Rename(String),
    Undo,
    Redo,
    Skills(Option<String>),
    Tools,
    Bundles(Option<String>),
    Curator(Option<String>),
    Agents,
    Fork(String),
    Compact(Option<String>),
    SkillInvoke { name: String, args: String },
    Upcoming(String),
}

impl SlashCommand {
    #[allow(dead_code, non_upper_case_globals)]
    pub(crate) const ModelShow: Self = Self::ModelShowScoped { global: false };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiExportOptions {
    pub(crate) path: Option<String>,
    pub(crate) format: SessionExportFormat,
    pub(crate) include: SessionExportIncludeSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiShareOptions {
    pub(crate) path: Option<String>,
    pub(crate) include: SessionExportIncludeSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlashMenuItem {
    pub(crate) command: String,
    pub(crate) description: String,
    pub(crate) upcoming: bool,
    pub(crate) aliases: Vec<SlashMenuAlias>,
    pub(crate) replacement: String,
    pub(crate) completion: String,
    pub(crate) configured_alias: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlashMenuAlias {
    pub(crate) alias: String,
    pub(crate) replacement: String,
    pub(crate) target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlashHelpSections {
    pub(crate) general: Vec<String>,
    pub(crate) commands: Vec<String>,
    pub(crate) custom_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EffectiveSlashConfig {
    pub(crate) aliases: BTreeMap<String, String>,
    pub(crate) keybinds: Vec<SlashKeybind>,
    pub(crate) leader_key: KeyChord,
    pub(crate) leader_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlashKeybind {
    pub(crate) command: String,
    pub(crate) sequence: SlashKeySequence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlashShortcutMatch {
    LeaderPrefix,
    Command(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlashKeySequence {
    Chord(KeyChord),
    Leader(KeyChord),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyChord {
    pub(crate) code: KeyCode,
    pub(crate) modifiers: KeyModifiers,
    pub(crate) display: String,
}

pub(crate) const DEFAULT_LEADER_KEY: &str = "ctrl+x";
pub(crate) const DEFAULT_LEADER_TIMEOUT_MS: u64 = 2000;
pub(crate) const OLD_DYNAMIC_SKILL_PREFIX: &str = "/skill:";
pub(crate) const OBSOLETE_SLASH_COMMAND_TOKENS: &[&str] =
    &["/models", "/thinking", "/raw", "/session", "/effort"];

impl Default for EffectiveSlashConfig {
    fn default() -> Self {
        Self {
            aliases: BTreeMap::new(),
            keybinds: Vec::new(),
            leader_key: KeyChord {
                code: KeyCode::Char('x'),
                modifiers: KeyModifiers::CONTROL,
                display: DEFAULT_LEADER_KEY.to_string(),
            },
            leader_timeout: Duration::from_millis(DEFAULT_LEADER_TIMEOUT_MS),
        }
    }
}

impl EffectiveSlashConfig {
    pub(crate) fn leader_timeout(&self) -> Duration {
        self.leader_timeout
    }

    pub(crate) fn shortcut_for_key(
        &self,
        key: &KeyEvent,
        leader_pending: bool,
    ) -> Option<SlashShortcutMatch> {
        if leader_pending {
            for keybind in &self.keybinds {
                if let SlashKeySequence::Leader(chord) = &keybind.sequence
                    && chord.matches(key)
                {
                    return Some(SlashShortcutMatch::Command(keybind.command.clone()));
                }
            }
        }
        if self.leader_key.matches(key) && self.keybinds.iter().any(SlashKeybind::uses_leader) {
            return Some(SlashShortcutMatch::LeaderPrefix);
        }
        self.keybinds.iter().find_map(|keybind| {
            if let SlashKeySequence::Chord(chord) = &keybind.sequence
                && chord.matches(key)
            {
                return Some(SlashShortcutMatch::Command(keybind.command.clone()));
            }
            None
        })
    }

    pub(crate) fn is_configured_alias_token(&self, line: &str) -> bool {
        let trimmed = line.trim();
        !trimmed.chars().any(char::is_whitespace) && self.aliases.contains_key(trimmed)
    }

    pub(crate) fn expand_alias_line(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        let (command, rest) = split_command_token(trimmed);
        let target = self.aliases.get(command)?;
        let rest = rest.trim();
        if rest.is_empty() {
            Some(target.clone())
        } else {
            Some(format!("{target} {rest}"))
        }
    }

    pub(crate) fn aliases_for(&self, canonical: &str) -> Vec<String> {
        self.aliases
            .iter()
            .filter(|(_, target)| canonical_command_token(target).as_deref() == Some(canonical))
            .map(|(alias, target)| {
                if target.trim() == canonical {
                    alias.clone()
                } else {
                    format!("{alias} -> {target}")
                }
            })
            .collect()
    }

    pub(crate) fn shortcuts_for(&self, canonical: &str) -> Vec<String> {
        self.keybinds
            .iter()
            .filter(|keybind| {
                canonical_command_token(&keybind.command).as_deref() == Some(canonical)
            })
            .map(|keybind| {
                if keybind.command.trim() == canonical {
                    keybind.sequence.display()
                } else {
                    format!("{} -> {}", keybind.sequence.display(), keybind.command)
                }
            })
            .collect()
    }
}

impl SlashKeybind {
    pub(crate) fn uses_leader(&self) -> bool {
        matches!(self.sequence, SlashKeySequence::Leader(_))
    }
}

impl SlashKeySequence {
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Chord(chord) => chord.display.clone(),
            Self::Leader(chord) => format!("<leader>{}", chord.display),
        }
    }
}

impl KeyChord {
    pub(crate) fn matches(&self, key: &KeyEvent) -> bool {
        self.code == normalize_key_code(&key.code)
            && self.modifiers == normalized_modifiers(key.modifiers)
    }
}

#[cfg(test)]
pub(crate) fn base_slash_menu_items() -> Vec<SlashMenuItem> {
    configured_slash_menu_items(&EffectiveSlashConfig::default())
}

pub(crate) fn configured_slash_menu_items(config: &EffectiveSlashConfig) -> Vec<SlashMenuItem> {
    SLASH_COMMANDS
        .iter()
        .map(|spec| SlashMenuItem {
            command: spec.canonical.to_string(),
            description: spec.summary.to_string(),
            upcoming: spec.status == CommandStatus::Upcoming,
            aliases: menu_aliases_for_spec(spec, config),
            replacement: spec.canonical.to_string(),
            completion: spec.canonical.to_string(),
            configured_alias: false,
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn format_slash_help(skill_count: Option<usize>) -> String {
    format_slash_help_with_config(skill_count, &EffectiveSlashConfig::default())
}

pub(crate) fn format_slash_help_with_config(
    skill_count: Option<usize>,
    config: &EffectiveSlashConfig,
) -> String {
    let sections = slash_help_sections_with_config(skill_count, config);
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

pub(crate) fn slash_help_sections_with_config(
    skill_count: Option<usize>,
    config: &EffectiveSlashConfig,
) -> SlashHelpSections {
    let mut general = vec![
        "Shortcuts".to_string(),
        "Enter - submit".to_string(),
        "Shift+Enter/Ctrl+Enter/Alt+Enter/Ctrl+J - insert newline".to_string(),
        "? - show shortcuts".to_string(),
        "Tab - complete slash command".to_string(),
        "Shift+Tab - cycle mode".to_string(),
        "Esc - close active UI or interrupt running work".to_string(),
        "Ctrl+C/Ctrl+D - quit or copy active selection".to_string(),
        "Ctrl+O - copy latest answer as Markdown".to_string(),
        "Ctrl+B - toggle sidebar".to_string(),
        "Ctrl+T - focus transcript; Esc returns to composer".to_string(),
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
            general.push(help_command_row(spec, config));
        }
    }

    let mut commands = Vec::new();
    for spec in SLASH_COMMANDS
        .iter()
        .filter(|spec| spec.group == CommandGroup::Commands)
    {
        debug_assert_eq!(spec.surface, CommandSurface::TuiSlash);
        commands.extend(help_command_rows(spec, config));
    }

    let custom_commands = match skill_count {
        Some(count) if count > 0 => {
            let mut rows = vec![format!(
                "{} - {} ({count} available)",
                CUSTOM_SKILL_COMMAND.usage, CUSTOM_SKILL_COMMAND.summary
            )];
            if let Some(detail) = CUSTOM_SKILL_COMMAND.help_detail {
                rows.push(format!("  {detail}"));
            }
            rows.extend(configured_custom_command_rows(config));
            rows
        }
        _ => {
            let rows = configured_custom_command_rows(config);
            if rows.is_empty() {
                vec!["No custom commands available".to_string()]
            } else {
                rows
            }
        }
    };

    SlashHelpSections {
        general,
        commands,
        custom_commands,
    }
}

pub(crate) fn help_command_row(spec: &SlashCommandSpec, config: &EffectiveSlashConfig) -> String {
    let mut aliases = spec
        .aliases
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    aliases.extend(config.aliases_for(spec.canonical));
    let aliases = if aliases.is_empty() {
        String::new()
    } else {
        format!(" (aliases: {})", aliases.join(", "))
    };
    let shortcuts = config.shortcuts_for(spec.canonical);
    let shortcuts = if shortcuts.is_empty() {
        String::new()
    } else {
        format!(" (shortcuts: {})", shortcuts.join(", "))
    };
    format!("{} - {}{}{}", spec.usage, spec.summary, aliases, shortcuts)
}

pub(crate) fn help_command_rows(
    spec: &SlashCommandSpec,
    config: &EffectiveSlashConfig,
) -> Vec<String> {
    let mut rows = vec![help_command_row(spec, config)];
    if let Some(detail) = spec.help_detail {
        rows.push(format!("  {detail}"));
    }
    rows
}

pub(crate) fn menu_aliases_for_spec(
    spec: &SlashCommandSpec,
    config: &EffectiveSlashConfig,
) -> Vec<SlashMenuAlias> {
    let mut aliases = spec
        .aliases
        .iter()
        .map(|alias| SlashMenuAlias {
            alias: (*alias).to_string(),
            replacement: (*alias).to_string(),
            target: None,
        })
        .collect::<Vec<_>>();
    aliases.extend(
        config
            .aliases
            .iter()
            .filter(|(_, target)| {
                canonical_command_token(target).as_deref() == Some(spec.canonical)
            })
            .map(|(alias, target)| SlashMenuAlias {
                alias: alias.clone(),
                replacement: alias.clone(),
                target: Some(target.clone()),
            }),
    );
    aliases
}

pub(crate) fn configured_custom_command_rows(config: &EffectiveSlashConfig) -> Vec<String> {
    let targets = config
        .aliases
        .values()
        .chain(config.keybinds.iter().map(|keybind| &keybind.command))
        .collect::<BTreeSet<_>>();
    targets
        .into_iter()
        .map(|target| configured_custom_command_row(target, config))
        .collect()
}

pub(crate) fn configured_custom_command_row(target: &str, config: &EffectiveSlashConfig) -> String {
    let summary = canonical_command_token(target)
        .and_then(|canonical| slash_command_spec(&canonical).map(|spec| spec.summary))
        .unwrap_or("configured slash command");
    let aliases = config
        .aliases
        .iter()
        .filter(|(_, candidate)| candidate.as_str() == target)
        .map(|(alias, _)| alias.as_str())
        .collect::<Vec<_>>();
    let aliases = if aliases.is_empty() {
        String::new()
    } else {
        format!(" (aliases: {})", aliases.join(", "))
    };
    let shortcuts = config
        .keybinds
        .iter()
        .filter(|keybind| keybind.command == target)
        .map(|keybind| keybind.sequence.display())
        .collect::<Vec<_>>();
    let shortcuts = if shortcuts.is_empty() {
        String::new()
    } else {
        format!(" (shortcuts: {})", shortcuts.join(", "))
    };
    format!("{target} - {summary}{aliases}{shortcuts}")
}

pub(crate) fn parse_effective_slash_config(root: &Value) -> Result<EffectiveSlashConfig> {
    let Some(tui) = root.get("tui") else {
        return Ok(EffectiveSlashConfig::default());
    };
    let object = tui
        .as_object()
        .ok_or_else(|| anyhow!("tui must be an object"))?;
    let leader_key = match object.get("leader_key") {
        Some(value) => parse_required_key_chord(value, "tui.leader_key")?,
        None => EffectiveSlashConfig::default().leader_key,
    };
    let leader_timeout = match object.get("leader_timeout_ms") {
        Some(value) => Duration::from_millis(
            value
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(|| anyhow!("tui.leader_timeout_ms must be a positive integer"))?,
        ),
        None => Duration::from_millis(DEFAULT_LEADER_TIMEOUT_MS),
    };
    let aliases = parse_configured_aliases(object.get("slash_aliases"))?;
    let keybinds = parse_configured_keybinds(object.get("slash_keybinds"))?;
    let config = EffectiveSlashConfig {
        aliases,
        keybinds,
        leader_key,
        leader_timeout,
    };
    validate_effective_slash_config(&config)?;
    Ok(config)
}

pub(crate) fn parse_configured_aliases(value: Option<&Value>) -> Result<BTreeMap<String, String>> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("tui.slash_aliases must be an object"))?;
    let mut aliases = BTreeMap::new();
    for (target, value) in object {
        let target = validate_configured_slash_target(target, "tui.slash_aliases")?;
        for alias in parse_alias_values(value, &format!("tui.slash_aliases.{target}"))? {
            if aliases.insert(alias.clone(), target.clone()).is_some() {
                return Err(anyhow!("duplicate slash alias: {alias}"));
            }
        }
    }
    Ok(aliases)
}

pub(crate) fn parse_alias_values(value: &Value, path: &str) -> Result<Vec<String>> {
    match value {
        Value::String(value) => Ok(vec![validate_configured_alias(value, path)?]),
        Value::Array(values) => values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let value = value
                    .as_str()
                    .ok_or_else(|| anyhow!("{path}[{index}] must be a string"))?;
                validate_configured_alias(value, &format!("{path}[{index}]"))
            })
            .collect(),
        _ => Err(anyhow!("{path} must be a string or array of strings")),
    }
}

pub(crate) fn validate_configured_alias(value: &str, path: &str) -> Result<String> {
    let alias = value.trim();
    if alias.is_empty() || !alias.starts_with('/') || alias.chars().any(char::is_whitespace) {
        return Err(anyhow!("{path} must be a slash alias without whitespace"));
    }
    if alias.starts_with(OLD_DYNAMIC_SKILL_PREFIX) {
        return Err(anyhow!(
            "{path} must not use the obsolete dynamic /skill: prefix"
        ));
    }
    Ok(alias.to_string())
}

pub(crate) fn parse_configured_keybinds(value: Option<&Value>) -> Result<Vec<SlashKeybind>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("tui.slash_keybinds must be an object"))?;
    let mut keybinds = Vec::new();
    for (target, value) in object {
        let target = validate_configured_slash_target(target, "tui.slash_keybinds")?;
        for sequence in parse_key_sequences(value, &format!("tui.slash_keybinds.{target}"))? {
            keybinds.push(SlashKeybind {
                command: target.clone(),
                sequence,
            });
        }
    }
    Ok(keybinds)
}

pub(crate) fn validate_configured_slash_target(value: &str, path: &str) -> Result<String> {
    let target = value.trim();
    if target.is_empty() || !target.starts_with('/') {
        return Err(anyhow!("{path} keys must be slash command lines"));
    }
    let (command, _) = split_command_token(target);
    if command.starts_with(OLD_DYNAMIC_SKILL_PREFIX) {
        return Err(anyhow!(
            "{path} does not support obsolete dynamic /skill: commands"
        ));
    }
    if slash_command_spec(command).is_none() && command != "/side" && command != "/reload-context" {
        return Err(anyhow!(
            "{path} target does not support dynamic skill or bundle commands"
        ));
    }
    parse_slash_command(target)
        .map_err(|err| anyhow!("{path} target {target:?} is invalid: {err:#}"))?
        .ok_or_else(|| anyhow!("{path} target {target:?} is not a slash command"))?;
    Ok(target.to_string())
}

pub(crate) fn parse_required_key_chord(value: &Value, path: &str) -> Result<KeyChord> {
    let value = value
        .as_str()
        .ok_or_else(|| anyhow!("{path} must be a string"))?;
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") || value.starts_with("<leader>") {
        return Err(anyhow!("{path} must be a single key chord"));
    }
    parse_key_chord(value, path)
}

pub(crate) fn parse_key_sequences(value: &Value, path: &str) -> Result<Vec<SlashKeySequence>> {
    let raw = match value {
        Value::String(value) => split_key_sequence_list(value),
        Value::Array(values) => {
            let mut items = Vec::new();
            for (index, value) in values.iter().enumerate() {
                let value = value
                    .as_str()
                    .ok_or_else(|| anyhow!("{path}[{index}] must be a string"))?;
                items.extend(split_key_sequence_list(value));
            }
            items
        }
        Value::Bool(false) => vec!["none".to_string()],
        _ => {
            return Err(anyhow!(
                "{path} must be a string, array of strings, or false"
            ));
        }
    };
    if raw.is_empty() {
        return Err(anyhow!("{path} must include at least one shortcut"));
    }
    if raw.iter().any(|value| value.eq_ignore_ascii_case("none")) {
        if raw.len() == 1 {
            return Ok(Vec::new());
        }
        return Err(anyhow!("{path} uses none with other shortcuts"));
    }
    raw.iter()
        .map(|value| parse_key_sequence(value, path))
        .collect()
}

pub(crate) fn split_key_sequence_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn parse_key_sequence(value: &str, path: &str) -> Result<SlashKeySequence> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("<leader>") {
        let rest = rest.strip_prefix('+').unwrap_or(rest).trim();
        if rest.is_empty() {
            return Err(anyhow!("{path} has an empty leader shortcut"));
        }
        return Ok(SlashKeySequence::Leader(parse_key_chord(rest, path)?));
    }
    Ok(SlashKeySequence::Chord(parse_key_chord(value, path)?))
}

pub(crate) fn parse_key_chord(value: &str, path: &str) -> Result<KeyChord> {
    let mut modifiers = KeyModifiers::empty();
    let mut key_name = None;
    for part in value
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
            "alt" | "meta" => modifiers |= KeyModifiers::ALT,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            _ if key_name.is_none() => key_name = Some(part.to_lowercase()),
            _ => return Err(anyhow!("{path} has invalid key chord {value:?}")),
        }
    }
    let key_name = key_name.ok_or_else(|| anyhow!("{path} has empty key chord"))?;
    let (code, modifiers) = key_code_from_name(&key_name, modifiers)
        .ok_or_else(|| anyhow!("{path} has unsupported key {key_name:?}"))?;
    Ok(KeyChord {
        code,
        modifiers,
        display: key_chord_display(&code, modifiers),
    })
}

pub(crate) fn key_code_from_name(
    name: &str,
    mut modifiers: KeyModifiers,
) -> Option<(KeyCode, KeyModifiers)> {
    let code = match name {
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" if modifiers.contains(KeyModifiers::SHIFT) => {
            modifiers.remove(KeyModifiers::SHIFT);
            KeyCode::BackTab
        }
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "space" => KeyCode::Char(' '),
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        value if value.len() == 1 => KeyCode::Char(value.chars().next()?),
        value if value.starts_with('f') => {
            let number = value.trim_start_matches('f').parse::<u8>().ok()?;
            if (1..=24).contains(&number) {
                KeyCode::F(number)
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some((code, modifiers))
}

pub(crate) fn key_chord_display(code: &KeyCode, modifiers: KeyModifiers) -> String {
    let mut parts = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_string());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".to_string());
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift".to_string());
    }
    parts.push(key_code_display(code));
    parts.join("+")
}

pub(crate) fn key_code_display(code: &KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(ch) => ch.to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => "shift+tab".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::F(number) => format!("f{number}"),
        other => format!("{other:?}").to_lowercase(),
    }
}

pub(crate) fn validate_effective_slash_config(config: &EffectiveSlashConfig) -> Result<()> {
    validate_alias_conflicts(config)?;
    validate_keybind_conflicts(config)
}

pub(crate) fn validate_alias_conflicts(config: &EffectiveSlashConfig) -> Result<()> {
    let mut reserved = SLASH_COMMANDS
        .iter()
        .flat_map(|spec| {
            std::iter::once(spec.canonical.to_string())
                .chain(spec.aliases.iter().map(|alias| (*alias).to_string()))
        })
        .collect::<BTreeSet<_>>();
    reserved.extend(
        OBSOLETE_SLASH_COMMAND_TOKENS
            .iter()
            .map(|value| (*value).to_string()),
    );
    let mut seen = BTreeSet::new();
    for alias in config.aliases.keys() {
        if reserved.contains(alias) {
            return Err(anyhow!(
                "slash alias conflicts with built-in command: {alias}"
            ));
        }
        if alias.starts_with(OLD_DYNAMIC_SKILL_PREFIX) {
            return Err(anyhow!(
                "slash alias conflicts with obsolete dynamic /skill: prefix: {alias}"
            ));
        }
        if !seen.insert(alias.clone()) {
            return Err(anyhow!("duplicate slash alias: {alias}"));
        }
    }
    Ok(())
}

pub(crate) fn validate_keybind_conflicts(config: &EffectiveSlashConfig) -> Result<()> {
    if fixed_key_chords()
        .iter()
        .any(|fixed| fixed == &config.leader_key)
    {
        return Err(anyhow!(
            "tui.leader_key conflicts with fixed key {}",
            config.leader_key.display
        ));
    }
    let mut seen = BTreeSet::new();
    for keybind in &config.keybinds {
        let display = keybind.sequence.display();
        if !seen.insert(display.clone()) {
            return Err(anyhow!("duplicate slash shortcut: {display}"));
        }
        match &keybind.sequence {
            SlashKeySequence::Chord(chord) => {
                if fixed_key_chords().iter().any(|fixed| fixed == chord) {
                    return Err(anyhow!(
                        "slash shortcut conflicts with fixed key: {display}"
                    ));
                }
                if chord == &config.leader_key {
                    return Err(anyhow!(
                        "slash shortcut conflicts with leader key: {display}"
                    ));
                }
            }
            SlashKeySequence::Leader(chord) => {
                if fixed_key_chords().iter().any(|fixed| fixed == chord) {
                    return Err(anyhow!(
                        "slash shortcut conflicts with fixed key: {display}"
                    ));
                }
            }
        }
    }
    Ok(())
}
