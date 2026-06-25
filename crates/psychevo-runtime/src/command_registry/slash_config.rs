use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::*;

pub const DEFAULT_SLASH_LEADER_KEY: &str = "ctrl+x";
pub const DEFAULT_SLASH_LEADER_TIMEOUT_MS: u64 = 2000;
pub const OLD_DYNAMIC_SKILL_PREFIX: &str = "/skill:";
pub const OBSOLETE_SLASH_COMMAND_TOKENS: &[&str] =
    &["/models", "/thinking", "/raw", "/session", "/effort"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedSlashConfig {
    pub leader_key: String,
    pub leader_timeout_ms: u64,
    pub aliases: Vec<SharedSlashAlias>,
    pub keybinds: Vec<SharedSlashKeybind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedSlashAlias {
    pub alias: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedSlashKeybind {
    pub shortcut: String,
    pub target: String,
}

impl Default for SharedSlashConfig {
    fn default() -> Self {
        Self {
            leader_key: DEFAULT_SLASH_LEADER_KEY.to_string(),
            leader_timeout_ms: DEFAULT_SLASH_LEADER_TIMEOUT_MS,
            aliases: Vec::new(),
            keybinds: Vec::new(),
        }
    }
}

impl SharedSlashConfig {
    pub fn expand_alias_line(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        let (command, rest) = split_slash_command_token(trimmed);
        let target = self
            .aliases
            .iter()
            .find(|entry| entry.alias == command)?
            .target
            .trim();
        let rest = rest.trim();
        if rest.is_empty() {
            Some(target.to_string())
        } else {
            Some(format!("{target} {rest}"))
        }
    }

    pub fn alias_map(&self) -> BTreeMap<String, String> {
        self.aliases
            .iter()
            .map(|entry| (entry.alias.clone(), entry.target.clone()))
            .collect()
    }
}

pub fn parse_shared_slash_config(root: &Value) -> crate::Result<SharedSlashConfig> {
    let Some(tui) = root.get("tui") else {
        return Ok(SharedSlashConfig::default());
    };
    let object = tui
        .as_object()
        .ok_or_else(|| crate::Error::Config("tui must be an object".to_string()))?;
    let leader_key = match object.get("leader_key") {
        Some(value) => parse_required_key_chord_display(value, "tui.leader_key")?,
        None => DEFAULT_SLASH_LEADER_KEY.to_string(),
    };
    let leader_timeout_ms = match object.get("leader_timeout_ms") {
        Some(value) => value.as_u64().filter(|value| *value > 0).ok_or_else(|| {
            crate::Error::Config("tui.leader_timeout_ms must be a positive integer".to_string())
        })?,
        None => DEFAULT_SLASH_LEADER_TIMEOUT_MS,
    };
    let aliases = parse_configured_alias_entries(object.get("slash_aliases"))?;
    let keybinds = parse_configured_keybind_entries(object.get("slash_keybinds"))?;
    let config = SharedSlashConfig {
        leader_key,
        leader_timeout_ms,
        aliases,
        keybinds,
    };
    validate_shared_slash_config(&config)?;
    Ok(config)
}

pub fn validate_shared_slash_config(config: &SharedSlashConfig) -> crate::Result<()> {
    validate_alias_conflicts(config)?;
    validate_keybind_conflicts(config)
}

pub fn parse_configured_alias_entries(
    value: Option<&Value>,
) -> crate::Result<Vec<SharedSlashAlias>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| crate::Error::Config("tui.slash_aliases must be an object".to_string()))?;
    let mut aliases = Vec::new();
    for (target, value) in object {
        let target = validate_configured_slash_target(target, "tui.slash_aliases")?;
        for alias in parse_alias_values(value, &format!("tui.slash_aliases.{target}"))? {
            aliases.push(SharedSlashAlias {
                alias,
                target: target.clone(),
            });
        }
    }
    Ok(aliases)
}

pub fn parse_configured_keybind_entries(
    value: Option<&Value>,
) -> crate::Result<Vec<SharedSlashKeybind>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| crate::Error::Config("tui.slash_keybinds must be an object".to_string()))?;
    let mut keybinds = Vec::new();
    for (target, value) in object {
        let target = validate_configured_slash_target(target, "tui.slash_keybinds")?;
        for shortcut in parse_key_sequences(value, &format!("tui.slash_keybinds.{target}"))? {
            keybinds.push(SharedSlashKeybind {
                shortcut,
                target: target.clone(),
            });
        }
    }
    Ok(keybinds)
}

pub fn validate_configured_alias(value: &str, path: &str) -> crate::Result<String> {
    let alias = value.trim();
    if alias.is_empty() || !alias.starts_with('/') || alias.chars().any(char::is_whitespace) {
        return Err(crate::Error::Config(format!(
            "{path} must be a slash alias without whitespace"
        )));
    }
    if alias.starts_with(OLD_DYNAMIC_SKILL_PREFIX) {
        return Err(crate::Error::Config(format!(
            "{path} must not use the obsolete dynamic /skill: prefix"
        )));
    }
    Ok(alias.to_string())
}

pub fn validate_configured_slash_target(value: &str, path: &str) -> crate::Result<String> {
    let target = value.trim();
    if target.is_empty() || !target.starts_with('/') {
        return Err(crate::Error::Config(format!(
            "{path} keys must be slash command lines"
        )));
    }
    let (command, _) = split_slash_command_token(target);
    if command.starts_with(OLD_DYNAMIC_SKILL_PREFIX) {
        return Err(crate::Error::Config(format!(
            "{path} does not support obsolete dynamic /skill: commands"
        )));
    }
    let SlashCommandParse::Known(invocation) = parse_slash_command_line(target) else {
        return Err(crate::Error::Config(format!(
            "{path} target does not support dynamic skill or bundle commands"
        )));
    };
    let effect = slash_invocation_effect(
        &invocation,
        &all_slash_capabilities(),
        SlashCommandSurface::Tui,
        false,
    )
    .map_err(|err| crate::Error::Config(format!("{path} target {target:?} is invalid: {err}")))?;
    match effect {
        SlashCommandEffect::Export { args } => {
            parse_session_export_command_args(
                args.as_deref().unwrap_or(""),
                SessionArtifactKind::Export,
                invocation.spec.usage,
            )
            .map_err(|err| {
                crate::Error::Config(format!("{path} target {target:?} is invalid: {err}"))
            })?;
        }
        SlashCommandEffect::Share { args } => {
            parse_session_export_command_args(
                args.as_deref().unwrap_or(""),
                SessionArtifactKind::Share,
                invocation.spec.usage,
            )
            .map_err(|err| {
                crate::Error::Config(format!("{path} target {target:?} is invalid: {err}"))
            })?;
        }
        _ => {}
    }
    Ok(target.to_string())
}

pub fn parse_key_sequence_display(value: &str, path: &str) -> crate::Result<String> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("<leader>") {
        let rest = rest.strip_prefix('+').unwrap_or(rest).trim();
        if rest.is_empty() {
            return Err(crate::Error::Config(format!(
                "{path} has an empty leader shortcut"
            )));
        }
        return Ok(format!("<leader>{}", parse_key_chord_display(rest, path)?));
    }
    parse_key_chord_display(value, path)
}

pub fn parse_key_chord_display(value: &str, path: &str) -> crate::Result<String> {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut key_name = None;
    for part in value
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "meta" => alt = true,
            "shift" => shift = true,
            _ if key_name.is_none() => key_name = Some(part.to_lowercase()),
            _ => {
                return Err(crate::Error::Config(format!(
                    "{path} has invalid key chord {value:?}"
                )));
            }
        }
    }
    let key_name =
        key_name.ok_or_else(|| crate::Error::Config(format!("{path} has empty key chord")))?;
    let key = normalized_key_name(&key_name)
        .ok_or_else(|| crate::Error::Config(format!("{path} has unsupported key {key_name:?}")))?;
    let mut parts = Vec::new();
    if ctrl {
        parts.push("ctrl".to_string());
    }
    if alt {
        parts.push("alt".to_string());
    }
    if shift && key != "shift+tab" {
        parts.push("shift".to_string());
    }
    parts.push(key);
    Ok(parts.join("+"))
}

pub fn fixed_key_chord_displays() -> &'static [&'static str] {
    &[
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
}

pub fn split_slash_command_token(line: &str) -> (&str, &str) {
    let trimmed = line.trim();
    match trimmed.find(char::is_whitespace) {
        Some(index) => (&trimmed[..index], &trimmed[index..]),
        None => (trimmed, ""),
    }
}

fn parse_alias_values(value: &Value, path: &str) -> crate::Result<Vec<String>> {
    match value {
        Value::String(value) => Ok(vec![validate_configured_alias(value, path)?]),
        Value::Array(values) => values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let value = value.as_str().ok_or_else(|| {
                    crate::Error::Config(format!("{path}[{index}] must be a string"))
                })?;
                validate_configured_alias(value, &format!("{path}[{index}]"))
            })
            .collect(),
        _ => Err(crate::Error::Config(format!(
            "{path} must be a string or array of strings"
        ))),
    }
}

fn parse_key_sequences(value: &Value, path: &str) -> crate::Result<Vec<String>> {
    let raw = match value {
        Value::String(value) => split_key_sequence_list(value),
        Value::Array(values) => {
            let mut items = Vec::new();
            for (index, value) in values.iter().enumerate() {
                let value = value.as_str().ok_or_else(|| {
                    crate::Error::Config(format!("{path}[{index}] must be a string"))
                })?;
                items.extend(split_key_sequence_list(value));
            }
            items
        }
        Value::Bool(false) => vec!["none".to_string()],
        _ => {
            return Err(crate::Error::Config(format!(
                "{path} must be a string, array of strings, or false"
            )));
        }
    };
    if raw.is_empty() {
        return Err(crate::Error::Config(format!(
            "{path} must include at least one shortcut"
        )));
    }
    if raw.iter().any(|value| value.eq_ignore_ascii_case("none")) {
        if raw.len() == 1 {
            return Ok(Vec::new());
        }
        return Err(crate::Error::Config(format!(
            "{path} uses none with other shortcuts"
        )));
    }
    raw.iter()
        .map(|value| parse_key_sequence_display(value, path))
        .collect()
}

fn parse_required_key_chord_display(value: &Value, path: &str) -> crate::Result<String> {
    let value = value
        .as_str()
        .ok_or_else(|| crate::Error::Config(format!("{path} must be a string")))?;
    if value.trim().eq_ignore_ascii_case("none") || value.trim().starts_with("<leader>") {
        return Err(crate::Error::Config(format!(
            "{path} must be a single key chord"
        )));
    }
    parse_key_chord_display(value, path)
}

pub fn split_key_sequence_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn validate_alias_conflicts(config: &SharedSlashConfig) -> crate::Result<()> {
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
            .map(|value| value.to_string()),
    );
    let mut seen = BTreeSet::new();
    for alias in config.aliases.iter().map(|entry| &entry.alias) {
        if reserved.contains(alias) {
            return Err(crate::Error::Config(format!(
                "slash alias conflicts with built-in command: {alias}"
            )));
        }
        if !seen.insert(alias.clone()) {
            return Err(crate::Error::Config(format!(
                "duplicate slash alias: {alias}"
            )));
        }
    }
    Ok(())
}

fn validate_keybind_conflicts(config: &SharedSlashConfig) -> crate::Result<()> {
    if fixed_key_chord_displays()
        .iter()
        .any(|fixed| fixed == &config.leader_key)
    {
        return Err(crate::Error::Config(format!(
            "tui.leader_key conflicts with fixed key {}",
            config.leader_key
        )));
    }
    let mut seen = BTreeSet::new();
    for keybind in &config.keybinds {
        if !seen.insert(keybind.shortcut.clone()) {
            return Err(crate::Error::Config(format!(
                "duplicate slash shortcut: {}",
                keybind.shortcut
            )));
        }
        if let Some(chord) = keybind.shortcut.strip_prefix("<leader>") {
            if fixed_key_chord_displays().contains(&chord) {
                return Err(crate::Error::Config(format!(
                    "slash shortcut conflicts with fixed key: {}",
                    keybind.shortcut
                )));
            }
        } else {
            if fixed_key_chord_displays()
                .iter()
                .any(|fixed| fixed == &keybind.shortcut)
            {
                return Err(crate::Error::Config(format!(
                    "slash shortcut conflicts with fixed key: {}",
                    keybind.shortcut
                )));
            }
            if keybind.shortcut == config.leader_key {
                return Err(crate::Error::Config(format!(
                    "slash shortcut conflicts with leader key: {}",
                    keybind.shortcut
                )));
            }
        }
    }
    Ok(())
}

fn normalized_key_name(name: &str) -> Option<String> {
    match name {
        "enter" | "return" => Some("enter".to_string()),
        "esc" | "escape" => Some("esc".to_string()),
        "tab" => Some("tab".to_string()),
        "backtab" => Some("shift+tab".to_string()),
        "space" => Some("space".to_string()),
        "backspace" => Some("backspace".to_string()),
        "delete" | "del" => Some("delete".to_string()),
        "home" => Some("home".to_string()),
        "end" => Some("end".to_string()),
        "pageup" | "pgup" => Some("pageup".to_string()),
        "pagedown" | "pgdn" => Some("pagedown".to_string()),
        "up" => Some("up".to_string()),
        "down" => Some("down".to_string()),
        "left" => Some("left".to_string()),
        "right" => Some("right".to_string()),
        value if value.len() == 1 => Some(value.to_string()),
        value if value.starts_with('f') => {
            let number = value.trim_start_matches('f').parse::<u8>().ok()?;
            if (1..=24).contains(&number) {
                Some(format!("f{number}"))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn all_slash_capabilities() -> Vec<CommandCapability> {
    vec![
        CommandCapability::Picker,
        CommandCapability::Clipboard,
        CommandCapability::RendererToggle,
        CommandCapability::ProcessExit,
        CommandCapability::SideConversation,
        CommandCapability::ImageAttachment,
        CommandCapability::ActiveTurnControl,
        CommandCapability::Queue,
        CommandCapability::SessionSwitch,
        CommandCapability::SessionRevert,
        CommandCapability::ArtifactWrite,
        CommandCapability::WorkspaceDiff,
        CommandCapability::ConfigWrite,
        CommandCapability::PolicyWrite,
        CommandCapability::SkillStateWrite,
    ]
}
