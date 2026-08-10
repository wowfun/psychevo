use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

use psychevo::{
    agents::AgentEntrypoint, agents::agent_source_display_label, skills::ListSkillsOptions,
    skills::list_skills_value_with_options, skills::skill_source_display_label,
};
use psychevo_gateway_protocol as wire;
use serde_json::Value;

use super::binding::WebState;
use super::commands::{command_item_completion_detail, command_item_matches, command_list_result};
use super::scope_session::ResolvedScope;
use super::settings_observability::{discover_gateway_agents, discover_gateway_skills};

const MAX_COMPLETION_ITEMS: usize = 50;
const MAX_FILE_COMPLETION_DEPTH: usize = 8;
const MAX_FILE_COMPLETION_VISITED_ENTRIES: usize = 4_000;
const MAX_CONCURRENT_FILE_COMPLETION_SCANS: usize = 4;
static FILE_COMPLETION_SCAN_ADMISSION: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| {
        Arc::new(tokio::sync::Semaphore::new(
            MAX_CONCURRENT_FILE_COMPLETION_SCANS,
        ))
    });

const GROUP_COMMANDS: &str = "commands";
const GROUP_SKILLS: &str = "skills";
const GROUP_AGENTS: &str = "agents";
const GROUP_DIRECTORIES: &str = "directories";
const GROUP_FILES: &str = "files";
const GROUP_CAPABILITIES: &str = "capabilities";
const GROUP_OPTIONS: &str = "options";

#[derive(Debug, Clone)]
pub(super) struct CompletionToken {
    pub(super) sigil: char,
    pub(super) query: String,
    pub(super) start: usize,
    pub(super) end: usize,
}

pub(super) async fn completion_list_value(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::thread_command_turn::CompletionListParams,
) -> psychevo::Result<Value> {
    let Some(token) = active_completion_token(&params.text, params.cursor) else {
        return Ok(serde_json::to_value(
            wire::thread_command_turn::CompletionListResult {
                items: Vec::new(),
                replacement: None,
            },
        )?);
    };
    let query = token.query.to_ascii_lowercase();
    let mut items = match token.sigil {
        '/' => slash_completion_items(state, scope, params.thread_id.as_deref(), &query).await?,
        '$' => dollar_completion_items(state, scope, &query)?,
        '@' => {
            at_completion_items(
                state,
                scope,
                params.thread_id.as_deref(),
                params.workspace_id.as_deref(),
                &query,
            )
            .await?
        }
        _ => Vec::new(),
    };
    items.truncate(MAX_COMPLETION_ITEMS);
    Ok(serde_json::to_value(
        wire::thread_command_turn::CompletionListResult {
            items,
            replacement: Some(wire::thread_command_turn::CompletionReplacement {
                start: token.start,
                end: token.end,
            }),
        },
    )?)
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

async fn slash_completion_items(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
    query: &str,
) -> psychevo::Result<Vec<wire::thread_command_turn::CompletionItem>> {
    let active_turn = match thread_id {
        Some(thread_id) => state.activity(&scope.source, Some(thread_id)).await.running,
        None => state.activity(&scope.source, None).await.running,
    };
    let mut commands = command_list_result(
        state,
        scope,
        active_turn,
        thread_id.is_some(),
        MAX_COMPLETION_ITEMS,
    )?
    .commands;
    commands.sort_by_key(|command| command_item_match_sort_key(command, query));
    let mut items = commands
        .into_iter()
        .filter(|command| command_item_matches(command, query))
        .map(|command| {
            let (group, group_label) = command_item_completion_group(&command);
            wire::thread_command_turn::CompletionItem {
                id: format!("command:{}", command.name),
                sigil: "/".to_string(),
                label: command.slash.clone(),
                insert_text: command.slash.clone(),
                kind: "command".to_string(),
                detail: Some(command_item_completion_detail(&command)),
                target: None,
                sort_text: Some(format!("command:{}", command.name)),
                group: Some(group.to_string()),
                group_label: Some(group_label.to_string()),
                scope_label: command_item_scope_label(&command),
            }
        })
        .collect::<Vec<_>>();
    items.sort_by_key(completion_group_rank);
    Ok(items)
}

fn command_item_match_sort_key(
    command: &wire::thread_command_turn::CommandListItem,
    query: &str,
) -> (u8, String) {
    if query.is_empty() {
        return (0, command.name.clone());
    }
    let query = query.to_ascii_lowercase();
    let name = command.name.to_ascii_lowercase();
    let slash = command.slash.trim_start_matches('/').to_ascii_lowercase();
    let expands_to = command
        .expands_to
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let score = if name == query || slash == query {
        0
    } else if name.starts_with(&query) || slash.starts_with(&query) {
        1
    } else if command
        .aliases
        .iter()
        .map(|alias| alias.trim_start_matches('/').to_ascii_lowercase())
        .any(|alias| alias == query)
    {
        2
    } else if command
        .aliases
        .iter()
        .map(|alias| alias.trim_start_matches('/').to_ascii_lowercase())
        .any(|alias| alias.starts_with(&query))
    {
        3
    } else if name.contains(&query) || slash.contains(&query) {
        4
    } else if expands_to.contains(&query) {
        5
    } else {
        6
    };
    (score, command.name.clone())
}

fn dollar_completion_items(
    state: &WebState,
    scope: &ResolvedScope,
    query: &str,
) -> psychevo::Result<Vec<wire::thread_command_turn::CompletionItem>> {
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
            items.push(wire::thread_command_turn::CompletionItem {
                id: format!("skill:{name}"),
                sigil: "$".to_string(),
                label: format!("${name}"),
                insert_text: format!("${name}"),
                kind: "skill".to_string(),
                detail: skill
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                target: Some(wire::source::GatewayMentionTarget::Skill {
                    name: name.to_string(),
                    path,
                }),
                sort_text: Some(completion_sort_text(
                    query,
                    name,
                    skill.get("description").and_then(Value::as_str),
                    "skill",
                )),
                group: Some(GROUP_SKILLS.to_string()),
                group_label: Some(completion_group_label(GROUP_SKILLS).to_string()),
                scope_label: skill_completion_scope_label(skill),
            });
        }
    }

    items.extend(agent_completion_items(state, scope, query, '$', None)?);
    items.sort_by(|left, right| {
        completion_group_rank(left)
            .cmp(&completion_group_rank(right))
            .then(left.sort_text.cmp(&right.sort_text))
            .then(left.label.cmp(&right.label))
    });
    Ok(items)
}

fn sort_grouped_completion_items(items: &mut [wire::thread_command_turn::CompletionItem]) {
    items.sort_by(compare_completion_items);
}

fn compare_completion_items(
    left: &wire::thread_command_turn::CompletionItem,
    right: &wire::thread_command_turn::CompletionItem,
) -> std::cmp::Ordering {
    completion_group_rank(left)
        .cmp(&completion_group_rank(right))
        .then(left.sort_text.cmp(&right.sort_text))
        .then(left.label.cmp(&right.label))
}

async fn at_completion_items(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: Option<&str>,
    workspace_id: Option<&str>,
    query: &str,
) -> psychevo::Result<Vec<wire::thread_command_turn::CompletionItem>> {
    let mut agents =
        agent_completion_items(state, scope, query, '@', Some(AgentEntrypoint::Subagent))?;
    let roots = if let Some(thread_id) = thread_id {
        state
            .inner
            .framework
            .thread_workspace_context(thread_id)
            .await?
            .roots
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>()
    } else if let Some(workspace_id) = workspace_id {
        state
            .inner
            .framework
            .workspace(workspace_id)
            .await?
            .ok_or_else(|| {
                psychevo::Error::Message(format!("Workspace `{workspace_id}` was not found"))
            })?
            .roots
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>()
    } else {
        vec![scope.cwd.clone()]
    };
    sort_grouped_completion_items(&mut agents);
    let roots_for_scan = roots.clone();
    let query_for_scan = query.to_string();
    let candidates =
        spawn_admitted_blocking(Arc::clone(&FILE_COMPLETION_SCAN_ADMISSION), move |cancel| {
            file_completion_candidates_with_cancellation(&roots_for_scan, &query_for_scan, &cancel)
        })
        .await
        .map_err(|error| {
            psychevo::Error::Message(format!("file completion worker failed: {error}"))
        })?;
    let reserved_file_slots = candidates.active_root_count().min(MAX_COMPLETION_ITEMS);
    agents.truncate(MAX_COMPLETION_ITEMS.saturating_sub(reserved_file_slots));
    let file_budget = MAX_COMPLETION_ITEMS.saturating_sub(agents.len());
    let mut items = agents;
    items.extend(candidates.select(file_budget));
    sort_grouped_completion_items(&mut items);
    Ok(items)
}

async fn spawn_admitted_blocking<T, F>(
    admission: Arc<tokio::sync::Semaphore>,
    task: F,
) -> Result<T, tokio::task::JoinError>
where
    T: Send + 'static,
    F: FnOnce(BlockingTaskCancellation) -> T + Send + 'static,
{
    let permit = admission
        .acquire_owned()
        .await
        .expect("file completion admission semaphore is never closed");
    let cancellation = BlockingTaskCancellation::default();
    let worker_cancellation = cancellation.clone();
    let _cancel_on_drop = CancelBlockingTaskOnDrop(cancellation);
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        task(worker_cancellation)
    })
    .await
}

#[derive(Clone, Default)]
struct BlockingTaskCancellation(Arc<AtomicBool>);

impl BlockingTaskCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

struct CancelBlockingTaskOnDrop(BlockingTaskCancellation);

impl Drop for CancelBlockingTaskOnDrop {
    fn drop(&mut self) {
        self.0.0.store(true, Ordering::Release);
    }
}

fn agent_completion_items(
    state: &WebState,
    scope: &ResolvedScope,
    query: &str,
    sigil: char,
    required_entrypoint: Option<AgentEntrypoint>,
) -> psychevo::Result<Vec<wire::thread_command_turn::CompletionItem>> {
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
        items.push(wire::thread_command_turn::CompletionItem {
            id: format!("agent:{name}"),
            sigil: sigil.to_string(),
            label: format!("{sigil}{name}"),
            insert_text: format!("{sigil}{name}"),
            kind: "agent".to_string(),
            detail: Some(description),
            target: Some(wire::source::GatewayMentionTarget::Agent {
                name,
                source: Some(agent.source.as_str().to_string()),
                entrypoints,
                backend_ref: agent.backend.map(|backend| backend.name),
            }),
            sort_text: Some(sort_text),
            group: Some(GROUP_AGENTS.to_string()),
            group_label: Some(completion_group_label(GROUP_AGENTS).to_string()),
            scope_label: agent_source_display_label(Some(agent.source.as_str()))
                .map(ToString::to_string),
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

struct FileCompletionCandidates {
    roots: Vec<Vec<wire::thread_command_turn::CompletionItem>>,
    #[cfg(test)]
    visited_entries: usize,
}

impl FileCompletionCandidates {
    fn active_root_count(&self) -> usize {
        self.roots.iter().filter(|items| !items.is_empty()).count()
    }

    fn select(self, limit: usize) -> Vec<wire::thread_command_turn::CompletionItem> {
        let mut unique =
            HashMap::<String, (usize, wire::thread_command_turn::CompletionItem)>::new();
        for (root_index, candidates) in self.roots.into_iter().enumerate() {
            for item in candidates {
                match unique.get_mut(&item.id) {
                    Some((existing_root, existing))
                        if compare_completion_items(&item, existing).is_lt() =>
                    {
                        *existing_root = root_index;
                        *existing = item;
                    }
                    Some(_) => {}
                    None => {
                        unique.insert(item.id.clone(), (root_index, item));
                    }
                }
            }
        }
        let mut ranked = unique.into_values().collect::<Vec<_>>();
        ranked.sort_by(|(_, left), (_, right)| compare_completion_items(left, right));
        let mut represented_roots = HashSet::new();
        let mut selected_ids = HashSet::new();
        let mut items = Vec::new();
        for (root_index, item) in &ranked {
            if represented_roots.insert(*root_index) {
                selected_ids.insert(item.id.clone());
                items.push(item.clone());
                if items.len() == limit {
                    sort_grouped_completion_items(&mut items);
                    return items;
                }
            }
        }
        for (_, item) in ranked {
            if selected_ids.insert(item.id.clone()) {
                items.push(item);
                if items.len() == limit {
                    break;
                }
            }
        }
        sort_grouped_completion_items(&mut items);
        items
    }
}

#[cfg(test)]
fn file_completion_candidates(roots: &[PathBuf], query: &str) -> FileCompletionCandidates {
    file_completion_candidates_with_cancellation(roots, query, &BlockingTaskCancellation::default())
}

fn file_completion_candidates_with_cancellation(
    roots: &[PathBuf],
    query: &str,
    cancellation: &BlockingTaskCancellation,
) -> FileCompletionCandidates {
    let multiple_roots = roots.len() > 1;
    let scope_labels = unique_root_scope_labels(roots);
    let mut root_items = Vec::new();
    #[cfg(test)]
    let mut visited_entries = 0;
    let mut remaining_budget = MAX_FILE_COMPLETION_VISITED_ENTRIES;
    for (index, (root, scope_label)) in roots.iter().zip(scope_labels.iter()).enumerate() {
        if cancellation.is_cancelled() {
            break;
        }
        let mut items = Vec::new();
        let remaining_roots = roots.len().saturating_sub(index).max(1);
        let allocated_budget = remaining_budget / remaining_roots;
        let mut visited_budget = allocated_budget;
        FileCompletionScan {
            root,
            query,
            scope_label: scope_label.as_deref(),
            multiple_roots,
            items: &mut items,
            visited_budget: &mut visited_budget,
            cancellation,
        }
        .collect(root, 0);
        #[cfg(test)]
        {
            visited_entries += allocated_budget - visited_budget;
        }
        remaining_budget = remaining_budget.saturating_sub(allocated_budget - visited_budget);
        sort_grouped_completion_items(&mut items);
        root_items.push(items);
    }
    FileCompletionCandidates {
        roots: root_items,
        #[cfg(test)]
        visited_entries,
    }
}

#[cfg(test)]
fn file_completion_items(
    roots: &[PathBuf],
    query: &str,
) -> psychevo::Result<Vec<wire::thread_command_turn::CompletionItem>> {
    Ok(file_completion_candidates(roots, query).select(MAX_COMPLETION_ITEMS))
}

fn unique_root_scope_labels(roots: &[PathBuf]) -> Vec<Option<String>> {
    if roots.len() <= 1 {
        return vec![None; roots.len()];
    }
    let components = roots
        .iter()
        .map(|root| {
            root.iter()
                .map(|component| component.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    components
        .iter()
        .enumerate()
        .map(|(index, parts)| {
            (1..=parts.len())
                .map(|count| parts[parts.len() - count..].join("/"))
                .find(|candidate| {
                    components.iter().enumerate().all(|(other_index, other)| {
                        other_index == index
                            || other.len() < candidate.split('/').count()
                            || other[other.len() - candidate.split('/').count()..].join("/")
                                != *candidate
                    })
                })
                .or_else(|| Some(roots[index].to_string_lossy().into_owned()))
        })
        .collect()
}

struct FileCompletionScan<'a> {
    root: &'a Path,
    query: &'a str,
    scope_label: Option<&'a str>,
    multiple_roots: bool,
    items: &'a mut Vec<wire::thread_command_turn::CompletionItem>,
    visited_budget: &'a mut usize,
    cancellation: &'a BlockingTaskCancellation,
}

impl FileCompletionScan<'_> {
    fn collect(&mut self, dir: &Path, depth: usize) {
        if self.cancellation.is_cancelled()
            || depth > MAX_FILE_COMPLETION_DEPTH
            || *self.visited_budget == 0
        {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if self.cancellation.is_cancelled() || *self.visited_budget == 0 {
                return;
            }
            *self.visited_budget -= 1;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if should_skip_completion_path(&name) {
                continue;
            }
            let Ok(relative) = path.strip_prefix(self.root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            let is_dir = path.is_dir();
            let label = if is_dir {
                format!("@{relative}/")
            } else {
                format!("@{relative}")
            };
            if self.query.is_empty() || relative.to_ascii_lowercase().contains(self.query) {
                let group = if is_dir {
                    GROUP_DIRECTORIES
                } else {
                    GROUP_FILES
                };
                self.retain_candidate(wire::thread_command_turn::CompletionItem {
                    id: format!("file:{}", path.display()),
                    sigil: "@".to_string(),
                    label: label.clone(),
                    insert_text: if self.multiple_roots {
                        format!("@{}{}", path.display(), if is_dir { "/" } else { "" })
                    } else {
                        label
                    },
                    kind: if is_dir { "directory" } else { "file" }.to_string(),
                    detail: Some(relative.clone()),
                    target: Some(wire::source::GatewayMentionTarget::File {
                        path: path.display().to_string(),
                        relative_path: relative.clone(),
                    }),
                    sort_text: Some(completion_sort_text(
                        self.query,
                        &relative,
                        None,
                        if is_dir { "directory" } else { "file" },
                    )),
                    group: Some(group.to_string()),
                    group_label: Some(completion_group_label(group).to_string()),
                    scope_label: self.scope_label.map(ToString::to_string),
                });
            }
            if is_dir {
                self.collect(&path, depth + 1);
            }
        }
    }

    fn retain_candidate(&mut self, item: wire::thread_command_turn::CompletionItem) {
        let index = self
            .items
            .binary_search_by(|current| compare_completion_items(current, &item))
            .unwrap_or_else(|index| index);
        if index >= MAX_COMPLETION_ITEMS {
            return;
        }
        self.items.insert(index, item);
        if self.items.len() > MAX_COMPLETION_ITEMS {
            self.items.pop();
        }
    }
}

fn command_item_completion_group(
    command: &wire::thread_command_turn::CommandListItem,
) -> (&'static str, &'static str) {
    let source = command.source.to_ascii_lowercase();
    let presentation = command
        .presentation_kind
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if source.contains("skill")
        || source == "dynamic"
        || presentation.contains("skill")
        || presentation.contains("extension")
    {
        return (GROUP_SKILLS, completion_group_label(GROUP_SKILLS));
    }
    if source.contains("capability") || presentation.contains("capability") {
        return (
            GROUP_CAPABILITIES,
            completion_group_label(GROUP_CAPABILITIES),
        );
    }
    (GROUP_COMMANDS, completion_group_label(GROUP_COMMANDS))
}

fn command_item_scope_label(
    command: &wire::thread_command_turn::CommandListItem,
) -> Option<String> {
    let (group, _) = command_item_completion_group(command);
    match group {
        GROUP_SKILLS => {
            let source = command.source.trim();
            if source == "dynamic" {
                Some("User".to_string())
            } else {
                skill_source_display_label(Some(source)).map(ToString::to_string)
            }
        }
        GROUP_CAPABILITIES => completion_scope_label(Some(command.source.as_str())),
        _ => None,
    }
}

fn skill_completion_scope_label(skill: &Value) -> Option<String> {
    skill
        .get("source_label")
        .and_then(Value::as_str)
        .and_then(|value| skill_source_display_label(Some(value)))
        .or_else(|| skill_source_display_label(skill.get("source").and_then(Value::as_str)))
        .map(ToString::to_string)
}

fn completion_scope_label(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn completion_group_label(group: &str) -> &'static str {
    match group {
        GROUP_COMMANDS => "Commands",
        GROUP_SKILLS => "Skills",
        GROUP_AGENTS => "Agents",
        GROUP_DIRECTORIES => "Directories",
        GROUP_FILES => "Files",
        GROUP_CAPABILITIES => "Capabilities",
        GROUP_OPTIONS => "Options",
        _ => "Options",
    }
}

fn completion_group_rank(item: &wire::thread_command_turn::CompletionItem) -> u8 {
    match item.group.as_deref().unwrap_or(item.kind.as_str()) {
        GROUP_COMMANDS | "command" => 0,
        GROUP_SKILLS | "skill" => 1,
        GROUP_AGENTS | "agent" => 2,
        GROUP_DIRECTORIES | "directory" => 3,
        GROUP_FILES | "file" => 4,
        GROUP_CAPABILITIES | "capability" => 5,
        GROUP_OPTIONS | "option" => 6,
        _ => 7,
    }
}

fn should_skip_completion_path(name: &str) -> bool {
    matches!(name, ".git" | ".local" | "target" | "node_modules")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::Duration;

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_completion_cooperatively_releases_blocking_scan_admission() {
        let admission = Arc::new(tokio::sync::Semaphore::new(1));
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let first = tokio::spawn(spawn_admitted_blocking(
            Arc::clone(&admission),
            move |cancel| {
                started_tx.send(()).expect("started");
                while !cancel.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(1));
                }
            },
        ));
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first scan started");
        first.abort();

        let second_started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let second_marker = Arc::clone(&second_started);
        let second = tokio::spawn(spawn_admitted_blocking(Arc::clone(&admission), move |_| {
            second_marker.store(true, std::sync::atomic::Ordering::SeqCst);
        }));
        tokio::time::timeout(Duration::from_secs(1), second)
            .await
            .expect("second scan admitted")
            .expect("join")
            .expect("scan");
        assert!(second_started.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn multi_root_file_completion_keeps_absolute_identity_and_labels_each_root() {
        let temp = tempfile::tempdir().expect("temp");
        let web = temp.path().join("web");
        let api = temp.path().join("api");
        std::fs::create_dir_all(&web).expect("web");
        std::fs::create_dir_all(&api).expect("api");
        std::fs::write(web.join("shared.ts"), "web").expect("web file");
        std::fs::write(api.join("shared.ts"), "api").expect("api file");

        let items =
            file_completion_items(&[web.clone(), api.clone()], "shared").expect("completion items");

        assert_eq!(items.len(), 2);
        let labels = items
            .iter()
            .map(|item| (item.id.as_str(), item.scope_label.as_deref()))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            labels.get(format!("file:{}", web.join("shared.ts").display()).as_str()),
            Some(&Some("web"))
        );
        assert_eq!(
            labels.get(format!("file:{}", api.join("shared.ts").display()).as_str()),
            Some(&Some("api"))
        );
        assert!(
            items
                .iter()
                .any(|item| item.id == format!("file:{}", web.join("shared.ts").display()))
        );
        assert!(
            items
                .iter()
                .any(|item| item.id == format!("file:{}", api.join("shared.ts").display()))
        );
        assert_eq!(
            items
                .iter()
                .map(|item| item.insert_text.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                format!("@{}", web.join("shared.ts").display()),
                format!("@{}", api.join("shared.ts").display()),
            ])
            .iter()
            .map(String::as_str)
            .collect()
        );
    }

    #[test]
    fn first_root_cannot_exhaust_the_multi_root_completion_budget() {
        let temp = tempfile::tempdir().expect("temp");
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir_all(&first).expect("first root");
        std::fs::create_dir_all(&second).expect("second root");
        for index in 0..(MAX_COMPLETION_ITEMS * 2) {
            std::fs::create_dir(first.join(format!("a-z-{index:03}")))
                .expect("first-root directory");
        }
        let exact = second.join("z");
        std::fs::write(&exact, "second").expect("second-root file");

        let items = file_completion_items(&[first, second], "z").expect("completion items");

        assert!(
            items
                .iter()
                .any(|item| item.id == format!("file:{}", exact.display())),
            "an exact match in a later root must survive the global limit"
        );
    }

    #[test]
    fn an_exact_match_after_the_fiftieth_root_survives_the_global_limit() {
        let temp = tempfile::tempdir().expect("temp");
        let mut roots = Vec::new();
        for index in 0..MAX_COMPLETION_ITEMS {
            let root = temp.path().join(format!("root-{index:03}"));
            std::fs::create_dir_all(&root).expect("root");
            std::fs::write(root.join(format!("weak-z-{index:03}.txt")), "weak")
                .expect("weak match");
            roots.push(root);
        }
        let exact_root = temp.path().join("root-exact");
        std::fs::create_dir_all(&exact_root).expect("exact root");
        let exact = exact_root.join("z");
        std::fs::write(&exact, "exact").expect("exact match");
        roots.push(exact_root);

        let items = file_completion_items(&roots, "z").expect("completion items");

        assert!(
            items
                .iter()
                .any(|item| item.id == format!("file:{}", exact.display())),
            "global ranking must inspect every root before applying the limit"
        );
    }

    #[test]
    fn nested_roots_emit_each_absolute_file_identity_once() {
        let temp = tempfile::tempdir().expect("temp");
        let repo = temp.path().join("repo");
        let nested = repo.join("sub");
        std::fs::create_dir_all(&nested).expect("nested root");
        let target = nested.join("shared.ts");
        std::fs::write(&target, "shared").expect("shared file");

        let items = file_completion_items(&[repo, nested], "shared").expect("completion items");

        assert_eq!(
            items
                .iter()
                .filter(|item| item.id == format!("file:{}", target.display()))
                .count(),
            1
        );
    }

    #[test]
    fn file_completion_traversal_has_a_global_entry_budget() {
        let temp = tempfile::tempdir().expect("temp");
        let roots = (0..2)
            .map(|root_index| {
                let root = temp.path().join(format!("root-{root_index}"));
                std::fs::create_dir_all(&root).expect("root");
                for index in 0..(MAX_FILE_COMPLETION_VISITED_ENTRIES / 2 + 100) {
                    std::fs::write(root.join(format!("entry-{index:05}.txt")), "entry")
                        .expect("entry");
                }
                root
            })
            .collect::<Vec<_>>();

        let candidates = file_completion_candidates(&roots, "never-matches");

        assert_eq!(
            candidates.visited_entries,
            MAX_FILE_COMPLETION_VISITED_ENTRIES
        );
    }

    #[test]
    fn sparse_roots_return_their_unused_scan_budget_to_later_roots() {
        let temp = tempfile::tempdir().expect("temp");
        let empty = temp.path().join("empty");
        let populated = temp.path().join("populated");
        std::fs::create_dir_all(&empty).expect("empty root");
        std::fs::create_dir_all(&populated).expect("populated root");
        for index in 0..3_000 {
            std::fs::write(populated.join(format!("entry-{index:05}.txt")), "entry")
                .expect("entry");
        }

        let candidates = file_completion_candidates(&[empty, populated], "never-matches");

        assert_eq!(candidates.visited_entries, 3_000);
        assert!(
            candidates
                .roots
                .iter()
                .all(|items| items.len() <= MAX_COMPLETION_ITEMS)
        );
    }

    #[test]
    fn empty_completion_keeps_only_a_bounded_candidate_set_per_root() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("root");
        std::fs::create_dir_all(&root).expect("root");
        for index in 0..200 {
            std::fs::write(root.join(format!("entry-{index:03}.txt")), "entry").expect("entry");
        }

        let candidates = file_completion_candidates(&[root], "");

        assert_eq!(candidates.roots[0].len(), MAX_COMPLETION_ITEMS);
    }

    #[test]
    fn multi_root_scope_labels_disambiguate_matching_basenames() {
        let roots = [
            PathBuf::from("/repo/apps/web"),
            PathBuf::from("/repo/services/web"),
        ];

        assert_eq!(
            unique_root_scope_labels(&roots),
            vec![
                Some("apps/web".to_string()),
                Some("services/web".to_string())
            ]
        );
    }
}
