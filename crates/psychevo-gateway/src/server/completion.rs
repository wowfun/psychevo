use std::path::Path;

use psychevo_gateway_protocol as wire;
use psychevo_runtime::command_registry::{
    AvailableSlashCommand, available_slash_commands_for_surface,
};
use psychevo_runtime::{AgentEntrypoint, ListSkillsOptions, list_skills_value_with_options};
use serde_json::Value;

use super::{
    ResolvedScope, WebState, command_completion_detail, discover_gateway_agents,
    discover_gateway_skills, dynamic_slash_commands, gateway_command_capabilities,
    web_desktop_command_visible,
};

const MAX_COMPLETION_ITEMS: usize = 50;
const MAX_FILE_COMPLETION_ITEMS: usize = 80;
const MAX_FILE_COMPLETION_DEPTH: usize = 8;

#[derive(Debug, Clone)]
pub(super) struct CompletionToken {
    pub(super) sigil: char,
    pub(super) query: String,
    pub(super) start: usize,
    pub(super) end: usize,
}

pub(super) fn completion_list_value(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::CompletionListParams,
) -> psychevo_runtime::Result<Value> {
    let Some(token) = active_completion_token(&params.text, params.cursor) else {
        return Ok(serde_json::to_value(wire::CompletionListResult {
            items: Vec::new(),
            replacement: None,
        })?);
    };
    let query = token.query.to_ascii_lowercase();
    let mut items = match token.sigil {
        '/' => slash_completion_items(state, scope, params.thread_id.as_deref(), &query)?,
        '$' => dollar_completion_items(state, scope, &query)?,
        '@' => at_completion_items(state, scope, &query)?,
        _ => Vec::new(),
    };
    items.truncate(MAX_COMPLETION_ITEMS);
    Ok(serde_json::to_value(wire::CompletionListResult {
        items,
        replacement: Some(wire::CompletionReplacement {
            start: token.start,
            end: token.end,
        }),
    })?)
}

pub(super) fn active_completion_token(text: &str, cursor: usize) -> Option<CompletionToken> {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let prefix = &text[..cursor];
    for (idx, ch) in prefix.char_indices().rev() {
        if ch.is_whitespace() {
            return None;
        }
        if !matches!(ch, '/' | '$' | '@') {
            continue;
        }
        if ch == '/' {
            let line_prefix = prefix[..idx].rsplit('\n').next().unwrap_or_default();
            if !line_prefix.trim().is_empty() {
                continue;
            }
        }
        let query = prefix[idx + ch.len_utf8()..].to_string();
        return Some(CompletionToken {
            sigil: ch,
            query,
            start: idx,
            end: cursor,
        });
    }
    None
}

fn slash_completion_items(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
    query: &str,
) -> psychevo_runtime::Result<Vec<wire::CompletionItem>> {
    let active_turn = thread_id
        .map(|thread_id| state.activity(&scope.source, Some(thread_id)).running)
        .unwrap_or_else(|| state.activity(&scope.source, None).running);
    let dynamic = dynamic_slash_commands(state, scope)?;
    let available = available_slash_commands_for_surface(
        &gateway_command_capabilities(thread_id.is_some()),
        active_turn,
        &dynamic,
        MAX_COMPLETION_ITEMS,
    );
    Ok(available
        .commands
        .into_iter()
        .filter(web_desktop_command_visible)
        .filter(|command| command_matches(command, query))
        .map(|command| wire::CompletionItem {
            id: format!("command:{}", command.name),
            sigil: "/".to_string(),
            label: format!("/{}", command.name),
            insert_text: format!("/{}", command.name),
            kind: "command".to_string(),
            detail: Some(command_completion_detail(&command)),
            target: None,
            sort_text: Some(format!("command:{}", command.name)),
        })
        .collect())
}

fn command_matches(command: &AvailableSlashCommand, query: &str) -> bool {
    query.is_empty()
        || command.name.contains(query)
        || command.aliases.iter().any(|alias| alias.contains(query))
        || command.summary.to_ascii_lowercase().contains(query)
}

fn dollar_completion_items(
    state: &WebState,
    scope: &ResolvedScope,
    query: &str,
) -> psychevo_runtime::Result<Vec<wire::CompletionItem>> {
    let mut items = Vec::new();
    let skill_catalog = discover_gateway_skills(state, scope)?;
    let skills = list_skills_value_with_options(
        &skill_catalog,
        &ListSkillsOptions {
            detail: true,
            enabled_only: true,
            ..ListSkillsOptions::default()
        },
    );
    if let Some(skills) = skills.get("skills").and_then(Value::as_array) {
        for skill in skills {
            let Some(name) = skill.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !completion_name_matches(
                name,
                skill.get("description").and_then(Value::as_str),
                query,
            ) {
                continue;
            }
            let path = skill
                .get("location")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            items.push(wire::CompletionItem {
                id: format!("skill:{name}"),
                sigil: "$".to_string(),
                label: format!("${name}"),
                insert_text: format!("${name}"),
                kind: "skill".to_string(),
                detail: skill
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                target: Some(wire::GatewayMentionTarget::Skill {
                    name: name.to_string(),
                    path,
                }),
                sort_text: Some(completion_sort_text(
                    query,
                    name,
                    skill.get("description").and_then(Value::as_str),
                    "skill",
                )),
            });
        }
    }

    items.extend(agent_completion_items(state, scope, query, '$', None)?);
    items.sort_by(|left, right| {
        left.sort_text
            .cmp(&right.sort_text)
            .then(left.label.cmp(&right.label))
    });
    Ok(items)
}

fn at_completion_items(
    state: &WebState,
    scope: &ResolvedScope,
    query: &str,
) -> psychevo_runtime::Result<Vec<wire::CompletionItem>> {
    let mut items =
        agent_completion_items(state, scope, query, '@', Some(AgentEntrypoint::Subagent))?;
    items.extend(file_completion_items(&scope.workdir, query)?);
    Ok(items)
}

fn agent_completion_items(
    state: &WebState,
    scope: &ResolvedScope,
    query: &str,
    sigil: char,
    required_entrypoint: Option<AgentEntrypoint>,
) -> psychevo_runtime::Result<Vec<wire::CompletionItem>> {
    let mut items = Vec::new();
    let agent_catalog = discover_gateway_agents(state, scope)?;
    for agent in agent_catalog.agents {
        if required_entrypoint.is_some_and(|entrypoint| !agent.supports_entrypoint(entrypoint)) {
            continue;
        }
        if !completion_name_matches(&agent.name, Some(&agent.description), query) {
            continue;
        }
        let name = agent.name.clone();
        let description = agent.description.clone();
        let sort_text = completion_sort_text(query, &name, Some(&description), "agent");
        let entrypoints = agent
            .entrypoints
            .iter()
            .map(|entrypoint| (*entrypoint).as_str().to_string())
            .collect::<Vec<_>>();
        items.push(wire::CompletionItem {
            id: format!("agent:{name}"),
            sigil: sigil.to_string(),
            label: format!("{sigil}{name}"),
            insert_text: format!("{sigil}{name}"),
            kind: "agent".to_string(),
            detail: Some(description),
            target: Some(wire::GatewayMentionTarget::Agent {
                name,
                source: Some(agent.source.as_str().to_string()),
                entrypoints,
                backend_ref: agent.backend.map(|backend| backend.name),
            }),
            sort_text: Some(sort_text),
        });
    }
    Ok(items)
}

fn completion_name_matches(name: &str, description: Option<&str>, query: &str) -> bool {
    query.is_empty()
        || name.to_ascii_lowercase().contains(query)
        || description.is_some_and(|description| description.to_ascii_lowercase().contains(query))
}

fn completion_sort_text(query: &str, name: &str, description: Option<&str>, kind: &str) -> String {
    let name_lower = name.to_ascii_lowercase();
    let description_lower = description.map(str::to_ascii_lowercase).unwrap_or_default();
    let rank = if query.is_empty() {
        2
    } else if name_lower == query {
        0
    } else if name_lower.starts_with(query) {
        1
    } else if name_lower
        .split(['-', '_', '/', '.'])
        .any(|part| part.starts_with(query))
    {
        2
    } else if name_lower.contains(query) {
        3
    } else if description_lower.contains(query) {
        4
    } else {
        9
    };
    format!("{rank}:{kind}:{name_lower}")
}

fn file_completion_items(
    workdir: &Path,
    query: &str,
) -> psychevo_runtime::Result<Vec<wire::CompletionItem>> {
    let mut items = Vec::new();
    collect_file_completion_items(workdir, workdir, query, 0, &mut items);
    items.sort_by(|left, right| left.label.cmp(&right.label));
    items.truncate(MAX_FILE_COMPLETION_ITEMS);
    Ok(items)
}

fn collect_file_completion_items(
    root: &Path,
    dir: &Path,
    query: &str,
    depth: usize,
    items: &mut Vec<wire::CompletionItem>,
) {
    if depth > MAX_FILE_COMPLETION_DEPTH || items.len() >= MAX_FILE_COMPLETION_ITEMS {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if items.len() >= MAX_FILE_COMPLETION_ITEMS {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if should_skip_completion_path(&name) {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let is_dir = path.is_dir();
        let label = if is_dir {
            format!("@{relative}/")
        } else {
            format!("@{relative}")
        };
        if query.is_empty() || relative.to_ascii_lowercase().contains(query) {
            items.push(wire::CompletionItem {
                id: format!("file:{relative}"),
                sigil: "@".to_string(),
                label: label.clone(),
                insert_text: label,
                kind: if is_dir { "directory" } else { "file" }.to_string(),
                detail: Some(relative.clone()),
                target: Some(wire::GatewayMentionTarget::File {
                    path: path.display().to_string(),
                    relative_path: relative.clone(),
                }),
                sort_text: Some(relative.clone()),
            });
        }
        if is_dir {
            collect_file_completion_items(root, &path, query, depth + 1, items);
        }
    }
}

fn should_skip_completion_path(name: &str) -> bool {
    matches!(name, ".git" | ".local" | "target" | "node_modules")
}
