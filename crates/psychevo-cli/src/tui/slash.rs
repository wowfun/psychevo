use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use anyhow::{Result, anyhow};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use psychevo_runtime::{
    SessionArtifactKind, SessionExportFormat, SessionExportIncludeSet, split_image_source_argument,
};
use serde_json::Value;

use crate::command_registry::{
    CUSTOM_SKILL_COMMAND, CommandArgumentKind, CommandGroup, CommandStatus, CommandSurface,
    SLASH_COMMANDS, SlashCommandAction, SlashCommandSpec, slash_command_spec,
};

pub(crate) const VARIANTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];
const GENERAL_COMMANDS: &[&str] = &[
    "/status",
    "/context",
    "/refresh",
    "/btw",
    "/model",
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
    Refresh,
    ReloadContextDeprecated,
    Btw(Option<String>),
    Steer(String),
    Queue(String),
    PendingCancel,
    ModelShow,
    VariantSet(String),
    ModeSet(String),
    Permissions,
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
    aliases: BTreeMap<String, String>,
    keybinds: Vec<SlashKeybind>,
    leader_key: KeyChord,
    leader_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlashKeybind {
    command: String,
    sequence: SlashKeySequence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlashShortcutMatch {
    LeaderPrefix,
    Command(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SlashKeySequence {
    Chord(KeyChord),
    Leader(KeyChord),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyChord {
    code: KeyCode,
    modifiers: KeyModifiers,
    display: String,
}

const DEFAULT_LEADER_KEY: &str = "ctrl+x";
const DEFAULT_LEADER_TIMEOUT_MS: u64 = 2000;
const OLD_DYNAMIC_SKILL_PREFIX: &str = "/skill:";
const OBSOLETE_SLASH_COMMAND_TOKENS: &[&str] =
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

    fn expand_alias_line(&self, line: &str) -> Option<String> {
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

    fn aliases_for(&self, canonical: &str) -> Vec<String> {
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

    fn shortcuts_for(&self, canonical: &str) -> Vec<String> {
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
    fn uses_leader(&self) -> bool {
        matches!(self.sequence, SlashKeySequence::Leader(_))
    }
}

impl SlashKeySequence {
    fn display(&self) -> String {
        match self {
            Self::Chord(chord) => chord.display.clone(),
            Self::Leader(chord) => format!("<leader>{}", chord.display),
        }
    }
}

impl KeyChord {
    fn matches(&self, key: &KeyEvent) -> bool {
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

fn help_command_row(spec: &SlashCommandSpec, config: &EffectiveSlashConfig) -> String {
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

fn help_command_rows(spec: &SlashCommandSpec, config: &EffectiveSlashConfig) -> Vec<String> {
    let mut rows = vec![help_command_row(spec, config)];
    if let Some(detail) = spec.help_detail {
        rows.push(format!("  {detail}"));
    }
    rows
}

fn menu_aliases_for_spec(
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

fn configured_custom_command_rows(config: &EffectiveSlashConfig) -> Vec<String> {
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

fn configured_custom_command_row(target: &str, config: &EffectiveSlashConfig) -> String {
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

fn parse_configured_aliases(value: Option<&Value>) -> Result<BTreeMap<String, String>> {
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

fn parse_alias_values(value: &Value, path: &str) -> Result<Vec<String>> {
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

fn validate_configured_alias(value: &str, path: &str) -> Result<String> {
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

fn parse_configured_keybinds(value: Option<&Value>) -> Result<Vec<SlashKeybind>> {
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

fn validate_configured_slash_target(value: &str, path: &str) -> Result<String> {
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

fn parse_required_key_chord(value: &Value, path: &str) -> Result<KeyChord> {
    let value = value
        .as_str()
        .ok_or_else(|| anyhow!("{path} must be a string"))?;
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") || value.starts_with("<leader>") {
        return Err(anyhow!("{path} must be a single key chord"));
    }
    parse_key_chord(value, path)
}

fn parse_key_sequences(value: &Value, path: &str) -> Result<Vec<SlashKeySequence>> {
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

fn split_key_sequence_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_key_sequence(value: &str, path: &str) -> Result<SlashKeySequence> {
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

fn parse_key_chord(value: &str, path: &str) -> Result<KeyChord> {
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

fn key_code_from_name(name: &str, mut modifiers: KeyModifiers) -> Option<(KeyCode, KeyModifiers)> {
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

fn key_chord_display(code: &KeyCode, modifiers: KeyModifiers) -> String {
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

fn key_code_display(code: &KeyCode) -> String {
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

fn validate_effective_slash_config(config: &EffectiveSlashConfig) -> Result<()> {
    validate_alias_conflicts(config)?;
    validate_keybind_conflicts(config)
}

fn validate_alias_conflicts(config: &EffectiveSlashConfig) -> Result<()> {
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

fn validate_keybind_conflicts(config: &EffectiveSlashConfig) -> Result<()> {
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

fn fixed_key_chords() -> Vec<KeyChord> {
    [
        "enter",
        "shift+enter",
        "ctrl+enter",
        "alt+enter",
        "ctrl+j",
        "esc",
        "ctrl+a",
        "ctrl+c",
        "ctrl+d",
        "ctrl+o",
        "ctrl+r",
        "ctrl+t",
        "ctrl+b",
        "tab",
        "shift+tab",
        "pageup",
        "pagedown",
        "up",
        "down",
        "home",
        "end",
        "shift+1",
        "shift+left",
        "alt+left",
        "alt+right",
        "alt+up",
        "alt+p",
    ]
    .iter()
    .filter_map(|value| parse_key_chord(value, "fixed").ok())
    .collect()
}

fn normalize_key_code(code: &KeyCode) -> KeyCode {
    match code {
        KeyCode::Char(ch) => KeyCode::Char(ch.to_ascii_lowercase()),
        other => *other,
    }
}

fn normalized_modifiers(modifiers: KeyModifiers) -> KeyModifiers {
    modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT)
}

fn split_command_token(line: &str) -> (&str, &str) {
    let trimmed = line.trim();
    match trimmed.find(char::is_whitespace) {
        Some(index) => (&trimmed[..index], &trimmed[index..]),
        None => (trimmed, ""),
    }
}

fn canonical_command_token(line: &str) -> Option<String> {
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

struct SlashItemMatch {
    score: u16,
    alias_index: usize,
    replacement: String,
    completion: String,
    command: String,
    description: Option<String>,
    configured_alias: bool,
}

fn best_slash_item_match(
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

fn parse_slash_command_inner(line: &str) -> Result<Option<SlashCommand>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('/') {
        return Ok(None);
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
                return Err(anyhow!("unknown slash command: {command}"));
            }
            if let Some(name) = dynamic_skill_name(command) {
                return Ok(Some(SlashCommand::SkillInvoke {
                    name,
                    args: rest.join(" "),
                }));
            }
            return Err(anyhow!("unknown slash command: {command}"));
        };
        parse_registered_slash_command(spec, command, &rest)?
    };
    Ok(Some(parsed))
}

fn dynamic_skill_name(command: &str) -> Option<String> {
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
        SlashCommandAction::Refresh => {
            parse_no_arguments(spec, command, rest)?;
            Ok(SlashCommand::Refresh)
        }
        SlashCommandAction::Btw => Ok(SlashCommand::Btw(parse_btw_prompt(rest))),
        SlashCommandAction::Steer => parse_required_trailing(spec, rest).map(SlashCommand::Steer),
        SlashCommandAction::Queue => parse_required_trailing(spec, rest).map(SlashCommand::Queue),
        SlashCommandAction::Pending => parse_pending_command(spec, rest),
        SlashCommandAction::ModelShow => {
            parse_no_arguments_for_usage(command, rest)?;
            Ok(SlashCommand::ModelShow)
        }
        SlashCommandAction::VariantSet => parse_variant_command(spec, rest),
        SlashCommandAction::ModeSet => parse_mode_command(spec, rest),
        SlashCommandAction::Permissions => {
            parse_no_arguments_for_usage(command, rest)?;
            Ok(SlashCommand::Permissions)
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
        SlashCommandAction::Compact => Ok(SlashCommand::Compact(parse_optional_trailing(rest))),
        SlashCommandAction::SkillInvoke => {
            unreachable!("dynamic skill commands are parsed before registry dispatch")
        }
    }
}

fn parse_no_arguments(spec: &SlashCommandSpec, command: &str, rest: &[&str]) -> Result<()> {
    debug_assert_eq!(spec.argument_kind, CommandArgumentKind::None);
    parse_no_arguments_for_usage(command, rest)
}

fn parse_no_arguments_for_usage(command: &str, rest: &[&str]) -> Result<()> {
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

fn parse_required_trailing(spec: &SlashCommandSpec, rest: &[&str]) -> Result<String> {
    let text = rest.join(" ");
    let text = text.trim();
    if text.is_empty() {
        return Err(anyhow!("usage: {}", spec.usage));
    }
    Ok(text.to_string())
}

fn parse_pending_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    match rest {
        ["cancel"] => Ok(SlashCommand::PendingCancel),
        _ => Err(anyhow!("usage: {}", spec.usage)),
    }
}

fn parse_export_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    let parsed = parse_export_like_options(spec, rest, true, SessionArtifactKind::Export)?;
    Ok(SlashCommand::Export(TuiExportOptions {
        path: parsed.path,
        format: parsed.format,
        include: parsed.include,
    }))
}

fn parse_share_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    let parsed = parse_export_like_options(spec, rest, false, SessionArtifactKind::Share)?;
    Ok(SlashCommand::Share(TuiShareOptions {
        path: parsed.path,
        include: parsed.include,
    }))
}

struct ParsedExportLikeOptions {
    path: Option<String>,
    format: SessionExportFormat,
    include: SessionExportIncludeSet,
}

fn parse_export_like_options(
    spec: &SlashCommandSpec,
    rest: &[&str],
    allow_format: bool,
    artifact_kind: SessionArtifactKind,
) -> Result<ParsedExportLikeOptions> {
    let tokens = split_slash_argument_tokens(&rest.join(" "))?;
    let mut path = None;
    let mut format = SessionExportFormat::Markdown;
    let mut include = None;
    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        match token.as_str() {
            "--include" | "-i" => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(anyhow!("usage: {}", spec.usage));
                };
                include = Some(parse_include(value, artifact_kind, spec)?);
            }
            "--format" | "-f" if allow_format => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(anyhow!("usage: {}", spec.usage));
                };
                format =
                    parse_export_format(value).ok_or_else(|| anyhow!("usage: {}", spec.usage))?;
            }
            value if allow_format && value.starts_with("--format=") => {
                let value = value.trim_start_matches("--format=");
                format =
                    parse_export_format(value).ok_or_else(|| anyhow!("usage: {}", spec.usage))?;
            }
            value if allow_format && value.starts_with("-f=") => {
                let value = value.trim_start_matches("-f=");
                format =
                    parse_export_format(value).ok_or_else(|| anyhow!("usage: {}", spec.usage))?;
            }
            value if value.starts_with("--include=") => {
                let value = value.trim_start_matches("--include=");
                include = Some(parse_include(value, artifact_kind, spec)?);
            }
            value if value.starts_with('-') => return Err(anyhow!("usage: {}", spec.usage)),
            value => {
                if path.is_some() {
                    return Err(anyhow!("usage: {}", spec.usage));
                }
                path = Some(value.to_string());
            }
        }
        index += 1;
    }
    Ok(ParsedExportLikeOptions {
        path,
        format,
        include: include.unwrap_or_else(|| SessionExportIncludeSet::default_for(artifact_kind)),
    })
}

fn parse_include(
    value: &str,
    artifact_kind: SessionArtifactKind,
    spec: &SlashCommandSpec,
) -> Result<SessionExportIncludeSet> {
    SessionExportIncludeSet::parse(value, artifact_kind)
        .map_err(|_| anyhow!("usage: {}", spec.usage))
}

fn parse_export_format(value: &str) -> Option<SessionExportFormat> {
    match value {
        "markdown" | "md" => Some(SessionExportFormat::Markdown),
        "json" => Some(SessionExportFormat::Json),
        _ => None,
    }
}

fn split_slash_argument_tokens(input: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(active), value) if value == active => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, value) if value.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            (_, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (_, value) => current.push(value),
        }
    }
    if quote.is_some() {
        return Err(anyhow!("unterminated quoted argument"));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
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

fn parse_fork_command(spec: &SlashCommandSpec, rest: &[&str]) -> Result<SlashCommand> {
    let prompt = rest.join(" ");
    if prompt.trim().is_empty() {
        return Err(anyhow!("usage: {}", spec.usage));
    }
    Ok(SlashCommand::Fork(prompt))
}

fn parse_optional_trailing(rest: &[&str]) -> Option<String> {
    let text = rest.join(" ");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn parse_btw_prompt(rest: &[&str]) -> Option<String> {
    let prompt = rest.join(" ");
    let prompt = prompt.trim();
    (!prompt.is_empty()).then(|| prompt.to_string())
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
        "plan" | "default" | "acceptEdits" | "accept-edits" | "dontAsk" | "dont-ask"
        | "bypassPermissions" | "bypass-permissions" => Ok(()),
        _ => Err(anyhow!(
            "mode must be one of plan, default, acceptEdits, dontAsk, bypassPermissions"
        )),
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
        assert_eq!(
            parse_slash_command("/refresh").unwrap(),
            Some(SlashCommand::Refresh)
        );
        assert_eq!(
            parse_slash_command("/reload-context").unwrap(),
            Some(SlashCommand::ReloadContextDeprecated)
        );
        assert_eq!(
            parse_slash_command("/btw").unwrap(),
            Some(SlashCommand::Btw(None))
        );
        assert_eq!(
            parse_slash_command("/btw explain this").unwrap(),
            Some(SlashCommand::Btw(Some("explain this".to_string())))
        );
        assert_eq!(
            parse_slash_command("/side explain this").unwrap(),
            Some(SlashCommand::Btw(Some("explain this".to_string())))
        );
        assert_eq!(
            parse_slash_command("/steer revise this").unwrap(),
            Some(SlashCommand::Steer("revise this".to_string()))
        );
        assert_eq!(
            parse_slash_command("/queue after this").unwrap(),
            Some(SlashCommand::Queue("after this".to_string()))
        );
        assert_eq!(
            parse_slash_command("/pending cancel").unwrap(),
            Some(SlashCommand::PendingCancel)
        );
        assert!(parse_slash_command("/steer").is_err());
        assert!(parse_slash_command("/queue").is_err());
        assert!(parse_slash_command("/pending").is_err());
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
                .contains("usage: /mode <plan|default|acceptEdits|dontAsk|bypassPermissions>")
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
    fn parses_export_share_commands() {
        assert_eq!(
            parse_slash_command("/export").unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: None,
                format: SessionExportFormat::Markdown,
                include: SessionExportIncludeSet::default_for(SessionArtifactKind::Export),
            }))
        );
        assert_eq!(
            parse_slash_command(
                "/export out.json --format json --include messages,reasoning,provider-input-evidence,last-provider-request"
            )
            .unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("out.json".to_string()),
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::parse(
                    "messages,reasoning,provider-input-evidence,last-provider-request",
                    SessionArtifactKind::Export,
                )
                .unwrap(),
            }))
        );
        assert_eq!(
            parse_slash_command("/export out.json --format json -i h,lpr").unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("out.json".to_string()),
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::parse("h,lpr", SessionArtifactKind::Export)
                    .unwrap(),
            }))
        );
        assert_eq!(
            parse_slash_command(r#"/export "session export.md" --format=markdown"#).unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("session export.md".to_string()),
                format: SessionExportFormat::Markdown,
                include: SessionExportIncludeSet::default_for(SessionArtifactKind::Export),
            }))
        );
        assert_eq!(
            parse_slash_command(
                "/share share.md --include messages,reasoning,provider-input-evidence"
            )
            .unwrap(),
            Some(SlashCommand::Share(TuiShareOptions {
                path: Some("share.md".to_string()),
                include: SessionExportIncludeSet::parse(
                    "messages,reasoning,provider-input-evidence",
                    SessionArtifactKind::Share,
                )
                .unwrap(),
            }))
        );
        assert_eq!(
            parse_slash_command("/export out.json -f json").unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("out.json".to_string()),
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::default_for(SessionArtifactKind::Export),
            }))
        );
        assert_eq!(
            parse_slash_command("/export out.json -f=json").unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("out.json".to_string()),
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::default_for(SessionArtifactKind::Export),
            }))
        );
        assert_eq!(
            parse_slash_command("/share share.md -i m,r,pie").unwrap(),
            Some(SlashCommand::Share(TuiShareOptions {
                path: Some("share.md".to_string()),
                include: SessionExportIncludeSet::parse("m,r,pie", SessionArtifactKind::Share)
                    .unwrap(),
            }))
        );
        assert!(parse_slash_command("/share --format json").is_err());
        assert!(parse_slash_command("/share -f json").is_err());
        assert!(parse_slash_command("/export --with-reasoning").is_err());
        assert!(parse_slash_command("/export --full-inputs").is_err());
        assert!(parse_slash_command("/export --last-request").is_err());
        assert!(parse_slash_command("/export --raw-requests").is_err());
        assert!(parse_slash_command("/share --with-reasoning").is_err());
        assert!(parse_slash_command("/share --full-inputs").is_err());
        assert!(parse_slash_command("/share --last-request").is_err());
        assert!(parse_slash_command("/share --raw-requests").is_err());
        assert!(parse_slash_command("/share --include last-provider-request").is_err());
        assert!(parse_slash_command("/export --format yaml").is_err());
        assert!(parse_slash_command("/export a b").is_err());
    }

    #[test]
    fn parses_skills_commands() {
        assert_eq!(
            parse_slash_command("/skills").unwrap(),
            Some(SlashCommand::Skills(None))
        );
        assert_eq!(
            parse_slash_command("/skills list").unwrap(),
            Some(SlashCommand::Skills(Some("list".to_string())))
        );
        assert_eq!(
            parse_slash_command("/reviewer extra context").unwrap(),
            Some(SlashCommand::SkillInvoke {
                name: "reviewer".to_string(),
                args: "extra context".to_string(),
            })
        );
        assert!(parse_slash_command("/skill:").is_err());
        assert!(parse_slash_command("/skill:reviewer").is_err());
    }

    #[test]
    fn configured_aliases_can_target_concrete_flagged_commands() {
        let config = parse_effective_slash_config(&serde_json::json!({
            "tui": {
                "slash_aliases": {
                    "/export -f json -i messages": ["/xj"]
                },
                "slash_keybinds": {
                    "/export -f json -i messages": "<leader>x"
                }
            }
        }))
        .expect("config");

        assert_eq!(
            parse_slash_command_with_config("/xj out.json", &config).unwrap(),
            Some(SlashCommand::Export(TuiExportOptions {
                path: Some("out.json".to_string()),
                format: SessionExportFormat::Json,
                include: SessionExportIncludeSet::parse("messages", SessionArtifactKind::Export)
                    .unwrap(),
            }))
        );

        let items = configured_slash_menu_items(&config);
        let matches = slash_menu_items_from("/x", &items);
        assert_eq!(matches[0].command, "/xj");
        assert!(
            matches[0]
                .description
                .contains("alias for /export -f json -i messages")
        );
        assert_eq!(matches[0].completion, "/xj");
        assert_eq!(matches[0].replacement, "/xj");

        let help = format_slash_help_with_config(Some(0), &config);
        assert!(help.contains("/xj -> /export -f json -i messages"));
        assert!(help.contains("<leader>x -> /export -f json -i messages"));
        let sections = slash_help_sections_with_config(Some(0), &config);
        assert_eq!(
            sections.custom_commands,
            vec![
                "/export -f json -i messages - write session export (aliases: /xj) (shortcuts: <leader>x)"
                    .to_string()
            ]
        );
    }

    #[test]
    fn configured_custom_command_help_rows_are_deduped() {
        let config = parse_effective_slash_config(&serde_json::json!({
            "tui": {
                "slash_aliases": {
                    "/status": ["/st", "/state"]
                },
                "slash_keybinds": {
                    "/status": ["alt+s", "<leader>s"]
                }
            }
        }))
        .expect("config");

        let sections = slash_help_sections_with_config(Some(2), &config);
        let configured_rows = sections
            .custom_commands
            .iter()
            .filter(|row| row.starts_with("/status - "))
            .collect::<Vec<_>>();
        assert_eq!(configured_rows.len(), 1);
        assert!(configured_rows[0].contains("(aliases: /st, /state)"));
        assert!(configured_rows[0].contains("(shortcuts: alt+s, <leader>s)"));
    }

    #[test]
    fn configured_slash_keybinds_match_direct_and_leader_sequences() {
        let config = parse_effective_slash_config(&serde_json::json!({
            "tui": {
                "leader_key": "ctrl+x",
                "slash_keybinds": {
                    "/status": "alt+s",
                    "/model": "<leader>m",
                    "/usage": "none"
                }
            }
        }))
        .expect("config");

        assert_eq!(
            config.shortcut_for_key(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT), false),
            Some(SlashShortcutMatch::Command("/status".to_string()))
        );
        assert_eq!(
            config.shortcut_for_key(
                &KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
                false
            ),
            Some(SlashShortcutMatch::LeaderPrefix)
        );
        assert_eq!(
            config.shortcut_for_key(&KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE), true),
            Some(SlashShortcutMatch::Command("/model".to_string()))
        );
        assert_eq!(
            config.shortcut_for_key(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::ALT), false),
            None
        );
    }

    #[test]
    fn configured_slash_conflicts_are_rejected() {
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_aliases": { "/status": ["/model"] }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("conflicts with built-in command")
        );
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_aliases": { "/not-real": ["/nr"] }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("target")
        );
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_keybinds": {
                        "/status": "alt+s",
                        "/model": "alt+s"
                    }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("duplicate slash shortcut")
        );
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_keybinds": { "/status": "enter" }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("fixed key")
        );
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_keybinds": { "/status": "ctrl+a" }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("fixed key")
        );
        assert!(
            parse_effective_slash_config(&serde_json::json!({
                "tui": {
                    "slash_keybinds": { "/status": "" }
                }
            }))
            .unwrap_err()
            .to_string()
            .contains("must include at least one shortcut")
        );
    }

    #[test]
    fn slash_menu_filters_and_marks_upcoming() {
        assert_eq!(slash_menu_items("/he")[0].command, "/help");
        assert_eq!(slash_menu_items("/usage")[0].command, "/usage");
        assert_eq!(slash_menu_items("/stats")[0].command, "/usage");
        assert_eq!(slash_prefix_menu_items("/stats")[0].command, "/usage");
        assert_eq!(slash_menu_items("/clear")[0].command, "/new");
        assert_eq!(slash_menu_items("/resume")[0].command, "/resume");
        assert_eq!(slash_menu_items("/refresh")[0].command, "/refresh");
        assert_eq!(slash_menu_items("/btw")[0].command, "/btw");
        assert!(slash_menu_items("/side").is_empty());
        assert!(slash_menu_items("/reload-context").is_empty());
        assert_eq!(slash_menu_items("/session").len(), 1);
        assert_eq!(slash_menu_items("/session")[0].command, "/sessions");
        assert!(slash_menu_items("/session ").is_empty());
        assert_eq!(slash_menu_items("/model").len(), 1);
        assert_eq!(slash_menu_items("/model")[0].command, "/model");
        assert_eq!(slash_menu_items("/model")[0].description, "choose model");
        let mode = slash_menu_items("/mode");
        assert_eq!(mode[0].command, "/mode");
        assert_eq!(mode[0].description, "set runtime permission mode");
        let variant = slash_menu_items("/var");
        assert_eq!(variant[0].command, "/variant");
        assert_eq!(variant[0].description, "set reasoning effort");
        let undo = slash_menu_items("/un");
        assert_eq!(undo[0].command, "/undo");
        assert!(!undo[0].upcoming);
        let rename = slash_menu_items("/ren");
        assert_eq!(rename[0].command, "/rename");
        assert_eq!(rename[0].description, "rename current session");
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
        assert_eq!(
            parse_slash_command("/compact").unwrap(),
            Some(SlashCommand::Compact(None))
        );
        assert_eq!(
            parse_slash_command("/compact focus on todos").unwrap(),
            Some(SlashCommand::Compact(Some("focus on todos".to_string())))
        );
        assert!(parse_slash_command("/undo now").is_err());
        assert!(parse_slash_command("/redo now").is_err());
        assert_eq!(slash_menu_items("/compact")[0].command, "/compact");
        assert!(!slash_menu_items("/compact")[0].upcoming);
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
        assert!(help.contains("/refresh - reload context and clean side sessions"));
        assert!(help.contains("/btw [prompt] - open a side conversation"));
        assert!(!help.contains("/reload-context"));
        assert!(!help.contains("/side"));
        assert!(help.contains("/usage - local usage and cost (aliases: /stats)"));
        assert!(help.contains("Reads persisted SQLite accounting and cost estimates"));
        assert!(help.contains("/sessions - list sessions"));
        assert!(help.contains("/resume [ref] - resume a previous session"));
        assert!(help.contains("/continue [ref] - continue a previous session"));
        assert!(help.contains("text surfaces list numbered sessions"));
        assert!(help.contains("Text surfaces accept latest, a listed number, an id prefix"));
        assert!(help.contains(
            "/export [path] [-f|--format markdown|json] [-i|--include list] - write session export"
        ));
        assert!(help.contains("last-provider-request can expose hidden prompts"));
        assert!(
            help.contains("/<skill-or-bundle> [args] - submit a skill or bundle (2 available)")
        );
        assert!(help.contains(
            "Submits the slash line while using the equivalent explicit skill or bundle marker"
        ));
        assert!(!help.contains("pevo run"));

        let empty = format_slash_help(Some(0));
        assert!(empty.contains("No custom commands available"));
    }

    #[test]
    fn slash_menu_can_filter_dynamic_skill_entries() {
        let mut items = base_slash_menu_items();
        items.push(SlashMenuItem {
            command: "/reviewer".to_string(),
            description: "Review code changes".to_string(),
            upcoming: false,
            aliases: Vec::new(),
            replacement: "/reviewer".to_string(),
            completion: "/reviewer".to_string(),
            configured_alias: false,
        });

        let matches = slash_menu_items_from("/rev", &items);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].command, "/reviewer");
        assert_eq!(matches[0].description, "Review code changes");
    }
}
