#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn parse_agent_file(path: &Path, source: AgentSource) -> Result<AgentDefinition> {
    let content = fs::read_to_string(path)?;
    let (frontmatter, instructions) = split_frontmatter(&content)?;
    let raw = match frontmatter {
        Some(frontmatter) => serde_yaml::from_str::<RawAgentFrontmatter>(frontmatter)
            .map_err(|err| Error::Config(format!("agent frontmatter failed: {err}")))?,
        None => RawAgentFrontmatter::default(),
    };
    let default_name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("agent");
    agent_from_raw(
        raw,
        default_name,
        instructions,
        Some(path.to_path_buf()),
        source,
    )
}

pub(crate) fn agent_from_raw(
    raw: RawAgentFrontmatter,
    default_name: &str,
    instructions: String,
    file_path: Option<PathBuf>,
    source: AgentSource,
) -> Result<AgentDefinition> {
    let path = file_path.clone();
    let name = raw
        .name
        .as_deref()
        .unwrap_or(default_name)
        .trim()
        .to_string();
    let mut diagnostics = Vec::new();
    if !valid_agent_name(&name) {
        diagnostics.push(AgentDiagnostic::warning(
            format!("agent name `{name}` is invalid"),
            path.clone(),
        ));
    }
    let description = raw
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config(format!("agent `{name}` must define a description")))?
        .to_string();

    if raw.memory.is_some() {
        diagnostics.push(AgentDiagnostic::warning(
            "agent memory is parsed but not executed in this version",
            path.clone(),
        ));
    }
    if raw
        .isolation
        .as_ref()
        .is_some_and(|value| value.as_str() == Some("worktree"))
    {
        diagnostics.push(AgentDiagnostic::warning(
            "agent isolation: worktree is parsed but not executed in this version",
            path.clone(),
        ));
    }
    if raw_declares_backend_details(&raw) {
        return Err(Error::Config(
            "agent markdown may only reference external backends with backend.ref; configure executable backend details under [agents.backends.<id>]".to_string(),
        ));
    }

    let (permission_mode, permission_diagnostic) =
        parse_permission_mode(raw.permission_mode.as_ref());
    if let Some(message) = permission_diagnostic {
        diagnostics.push(AgentDiagnostic::warning(message, path.clone()));
    }
    let (project_instructions, project_instructions_diagnostic) =
        parse_project_instructions(raw.project_instructions.as_ref());
    if let Some(message) = project_instructions_diagnostic {
        diagnostics.push(AgentDiagnostic::warning(message, path.clone()));
    }
    let tool_policy = parse_agent_tool_policy(
        raw.tools.as_ref(),
        raw.disallowed_tools.as_ref(),
        raw.permissions.or(raw.permission),
        permission_mode,
        raw.mcp_servers.as_ref(),
    );
    diagnostics.extend(tool_policy_diagnostics(&tool_policy, path.clone()));
    let model = raw.model.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty() && trimmed != "inherit").then(|| trimmed.to_string())
    });
    let backend = parse_agent_backend_ref(raw.backend.as_ref())?;
    let entrypoints = parse_agent_entrypoints(raw.entrypoints.as_ref(), backend.is_some())?;

    Ok(AgentDefinition {
        name,
        description,
        instructions: instructions.trim().to_string(),
        file_path,
        source,
        backend,
        entrypoints,
        model,
        tool_policy,
        skills: parse_string_vec(raw.skills.as_ref()),
        hooks: raw.hooks,
        background: raw.background,
        initial_prompt: raw
            .initial_prompt
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        max_turns: raw.max_turns,
        max_spawn_depth: clamp_agent_spawn_depth(raw.max_spawn_depth),
        project_instructions,
        effort: raw.effort,
        diagnostics,
    })
}

pub(crate) fn raw_declares_backend_details(raw: &RawAgentFrontmatter) -> bool {
    raw.kind.is_some()
        || raw.enabled.is_some()
        || raw.label.is_some()
        || raw.command.is_some()
        || raw.args.is_some()
        || raw.env.is_some()
        || raw.client_capabilities.is_some()
        || raw.cwd.is_some()
}

pub(crate) fn parse_agent_backend_ref(value: Option<&Value>) -> Result<Option<AgentBackendRef>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let object = value.as_object().ok_or_else(|| {
        Error::Config("agent backend must be an object with backend.ref".to_string())
    })?;
    let Some(raw_ref) = object.get("ref") else {
        return Err(Error::Config(
            "agent backend must define backend.ref".to_string(),
        ));
    };
    if object.keys().any(|key| key != "ref") {
        return Err(Error::Config(
            "agent backend only supports backend.ref in Markdown definitions".to_string(),
        ));
    }
    let name = raw_ref
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config("agent backend.ref must be a non-empty string".to_string()))?;
    if !valid_agent_name(name) {
        return Err(Error::Config(format!(
            "agent backend.ref `{name}` is invalid"
        )));
    }
    Ok(Some(AgentBackendRef {
        name: name.to_string(),
    }))
}

pub(crate) fn parse_agent_entrypoints(
    value: Option<&Value>,
    backend_ref: bool,
) -> Result<BTreeSet<AgentEntrypoint>> {
    let Some(value) = value else {
        return Ok(if backend_ref {
            default_peer_agent_entrypoints()
        } else {
            default_subagent_entrypoints()
        });
    };
    let values = match value {
        Value::String(raw) => raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        Error::Config("agent entrypoints entries must be strings".to_string())
                    })
            })
            .collect::<Result<Vec<_>>>()?,
        _ => {
            return Err(Error::Config(
                "agent entrypoints must be a string or array".to_string(),
            ));
        }
    };
    if values.is_empty() {
        return Err(Error::Config(
            "agent entrypoints must include at least one value".to_string(),
        ));
    }
    let mut entrypoints = BTreeSet::new();
    for value in values {
        let entrypoint = AgentEntrypoint::parse(&value).ok_or_else(|| {
            Error::Config(format!(
                "agent entrypoint `{value}` must be peer or subagent"
            ))
        })?;
        entrypoints.insert(entrypoint);
    }
    Ok(entrypoints)
}

pub(crate) fn clamp_agent_spawn_depth(value: Option<u8>) -> u8 {
    value.unwrap_or(0).min(MAX_AGENT_SPAWN_DEPTH_CAP)
}

pub(crate) fn split_frontmatter(content: &str) -> Result<(Option<&str>, String)> {
    let Some(rest) = content.strip_prefix("---\n") else {
        return Ok((None, content.to_string()));
    };
    let Some(end) = rest.find("\n---") else {
        return Err(Error::Config("agent frontmatter is not closed".to_string()));
    };
    let frontmatter = &rest[..end];
    let body = rest[end + "\n---".len()..]
        .strip_prefix('\n')
        .unwrap_or(&rest[end + "\n---".len()..]);
    Ok((Some(frontmatter), body.to_string()))
}

pub(crate) fn parse_agent_tool_policy(
    tools: Option<&Value>,
    disallowed_tools: Option<&Value>,
    permissions: Option<Value>,
    permission_mode: Option<AgentPermissionMode>,
    mcp_servers: Option<&Value>,
) -> AgentToolPolicy {
    let allowed = parse_allowed_tool_entries(tools);
    let denied = parse_tool_entries(disallowed_tools, ToolEntryMode::Deny);
    let (allowed_tools, allowed_agents) = match allowed {
        Some(allowed) => (
            Some(allowed.tools),
            (!allowed.agents.is_empty()).then_some(allowed.agents),
        ),
        None => (None, None),
    };
    AgentToolPolicy {
        allowed: allowed_tools,
        denied: denied.tools,
        allowed_agents,
        denied_agents: denied.agents,
        permissions,
        permission_mode,
        mcp_servers: parse_mcp_server_set(mcp_servers),
    }
}

pub(crate) fn parse_allowed_tool_entries(value: Option<&Value>) -> Option<ParsedToolEntries> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) if raw.trim().is_empty() => None,
        Some(Value::Array(items)) if items.is_empty() => Some(ParsedToolEntries::default()),
        Some(_) => Some(parse_tool_entries(value, ToolEntryMode::Allow)),
    }
}

pub(crate) fn tool_policy_diagnostics(
    policy: &AgentToolPolicy,
    path: Option<PathBuf>,
) -> Vec<AgentDiagnostic> {
    let mut diagnostics = Vec::new();
    for tool in policy
        .allowed
        .iter()
        .flat_map(|tools| tools.iter())
        .chain(policy.denied.iter())
    {
        if !known_tool_policy_name(tool) {
            diagnostics.push(AgentDiagnostic::warning(
                format!(
                    "agent tool `{tool}` is not a known built-in tool and will not match a built-in tool"
                ),
                path.clone(),
            ));
        }
    }
    diagnostics
}

pub(crate) fn known_tool_policy_name(name: &str) -> bool {
    matches!(
        name,
        "read"
            | "exec_command"
            | "write_stdin"
            | "edit"
            | "write"
            | "clarify"
            | "spawn_agent"
            | "Skill"
            | "list_agents"
            | "wait_agent"
            | "send_message"
            | "close_agent"
            | "resume_agent"
            | "list_skills"
            | "view_skill"
            | "skill_manage"
            | "skill_hub"
            | "skill_config"
    ) || mcp_tool_server(name).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolEntryMode {
    Allow,
    Deny,
}

#[derive(Debug, Default)]
pub(crate) struct ParsedToolEntries {
    pub(crate) tools: BTreeSet<String>,
    pub(crate) agents: BTreeSet<String>,
}

pub(crate) fn parse_tool_entries(value: Option<&Value>, mode: ToolEntryMode) -> ParsedToolEntries {
    let mut parsed = ParsedToolEntries::default();
    for item in parse_tool_vec(value) {
        let (tool, agents) = parse_tool_entry(&item);
        let canonical = normalize_tool_name(tool);
        if !(mode == ToolEntryMode::Deny && canonical == "spawn_agent" && !agents.is_empty()) {
            parsed.tools.insert(canonical.clone());
        }
        if canonical == "spawn_agent" {
            parsed.agents.extend(agents);
        }
    }
    parsed
}

pub(crate) fn parse_tool_vec(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(raw)) => split_tool_string(raw),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .flat_map(split_tool_string)
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn split_tool_string(raw: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in raw.chars() {
        match ch {
            '(' => {
                depth = depth.saturating_add(1);
                current.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => {
                let item = current.trim();
                if !item.is_empty() {
                    items.push(item.to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let item = current.trim();
    if !item.is_empty() {
        items.push(item.to_string());
    }
    items
}

pub(crate) fn parse_tool_entry(raw: &str) -> (String, Vec<String>) {
    let trimmed = raw.trim();
    let Some(open) = trimmed.find('(') else {
        return (trimmed.to_string(), Vec::new());
    };
    if !trimmed.ends_with(')') {
        return (trimmed.to_string(), Vec::new());
    }
    let tool = trimmed[..open].trim().to_string();
    let names = trimmed[open + 1..trimmed.len().saturating_sub(1)]
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    (tool, names)
}

pub(crate) fn parse_permission_mode(
    value: Option<&Value>,
) -> (Option<AgentPermissionMode>, Option<String>) {
    let Some(raw) = value.and_then(Value::as_str).map(str::trim) else {
        return (None, None);
    };
    match raw {
        "" | "default" => (Some(AgentPermissionMode::Default), None),
        "acceptEdits" | "accept_edits" => (Some(AgentPermissionMode::AcceptEdits), None),
        "plan" => (Some(AgentPermissionMode::Plan), None),
        "bypass"
        | "bypassPermissions"
        | "bypass_permissions"
        | "dangerous"
        | "dangerouslySkipPermissions"
        | "dangerously_skip_permissions" => (
            None,
            Some(format!(
                "permissionMode `{raw}` is parsed but unsupported; it does not expand tool access"
            )),
        ),
        other => (
            None,
            Some(format!(
                "permissionMode `{other}` is not recognized and does not change tool access"
            )),
        ),
    }
}

pub(crate) fn parse_project_instructions(value: Option<&Value>) -> (Option<bool>, Option<String>) {
    match value {
        None | Some(Value::Null) => (None, None),
        Some(Value::Bool(enabled)) => (Some(*enabled), None),
        Some(_) => (
            None,
            Some(
                "projectInstructions must be a boolean when set; defaulting to injected project instructions"
                    .to_string(),
            ),
        ),
    }
}

pub(crate) fn parse_string_set(value: Option<&Value>) -> Option<BTreeSet<String>> {
    let items = parse_string_vec(value);
    (!items.is_empty()).then(|| items.into_iter().collect())
}

pub(crate) fn parse_mcp_server_set(value: Option<&Value>) -> BTreeSet<String> {
    parse_string_set(value).unwrap_or_default()
}

pub(crate) fn parse_string_vec(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(raw)) => raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn normalize_tool_name(raw: String) -> String {
    match raw.trim() {
        "Read" | "read" => "read".to_string(),
        "ExecCommand" | "exec_command" => "exec_command".to_string(),
        "WriteStdin" | "write_stdin" => "write_stdin".to_string(),
        "Edit" | "edit" => "edit".to_string(),
        "Write" | "write" => "write".to_string(),
        "Clarify" | "clarify" => "clarify".to_string(),
        "spawn_agent" => "spawn_agent".to_string(),
        "Skill" | "skill" => "Skill".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn agent_allows_tool(
    name: &str,
    agent: Option<&AgentDefinition>,
    mode: RunMode,
) -> bool {
    let Some(agent) = agent else {
        if mode == RunMode::Plan && !plan_mode_tool_allowed(name) {
            return false;
        }
        return true;
    };
    if (mode == RunMode::Plan
        || agent.tool_policy.permission_mode == Some(AgentPermissionMode::Plan))
        && !plan_mode_tool_allowed(name)
    {
        return false;
    }
    let canonical = normalize_tool_name(name.to_string());
    let policy_names = tool_policy_names(name, &canonical);
    if policy_names
        .iter()
        .any(|name| agent.tool_policy.denied.contains(name.as_str()))
    {
        return false;
    }
    if let Some(server) = mcp_tool_server(name)
        && !agent.tool_policy.mcp_servers.is_empty()
        && !agent.tool_policy.mcp_servers.contains(server)
    {
        return false;
    }
    match &agent.tool_policy.allowed {
        Some(allowed) => policy_names
            .iter()
            .any(|name| allowed.contains(name.as_str())),
        None => true,
    }
}

pub(crate) fn tool_policy_names(name: &str, canonical: &str) -> Vec<String> {
    let mut names = Vec::from([canonical.to_string(), name.to_string()]);
    if agent_control_tool_name(name) {
        names.push("spawn_agent".to_string());
    }
    if skill_read_tool_name(name) {
        names.push("Skill".to_string());
    }
    names.sort();
    names.dedup();
    names
}

pub(crate) fn plan_mode_tool_allowed(name: &str) -> bool {
    matches!(
        name,
        "read"
            | "exec_command"
            | "write_stdin"
            | "clarify"
            | "list_skills"
            | "view_skill"
            | "skill_hub"
            | "skill_config"
            | "spawn_agent"
            | "list_agents"
            | "wait_agent"
            | "send_message"
            | "close_agent"
            | "resume_agent"
    )
}

pub(crate) fn mcp_tool_server(name: &str) -> Option<&str> {
    if let Some(raw) = name.strip_prefix("mcp:") {
        return raw
            .split([':', '/'])
            .next()
            .filter(|server| !server.is_empty());
    }
    name.strip_prefix("mcp__")
        .and_then(|raw| raw.split("__").next())
        .filter(|server| !server.is_empty())
}

pub(crate) fn agent_control_tool_name(name: &str) -> bool {
    matches!(
        name,
        "spawn_agent"
            | "list_agents"
            | "wait_agent"
            | "send_message"
            | "close_agent"
            | "resume_agent"
    )
}

pub(crate) fn skill_read_tool_name(name: &str) -> bool {
    matches!(name, "list_skills" | "view_skill")
}

pub(crate) fn agent_policy_allows_agent_catalog(agent: &AgentDefinition) -> bool {
    if agent.tool_policy.denied.contains("spawn_agent") {
        return false;
    }
    match &agent.tool_policy.allowed {
        Some(allowed) => allowed.contains("spawn_agent"),
        None => true,
    }
}

pub(crate) fn agent_policy_allows_skill_catalog(agent: &AgentDefinition) -> bool {
    if agent.tool_policy.denied.contains("Skill")
        || agent.tool_policy.denied.contains("list_skills")
        || agent.tool_policy.denied.contains("view_skill")
        || agent.tool_policy.denied.contains("skill_hub")
        || agent.tool_policy.denied.contains("skill_config")
    {
        return false;
    }
    match &agent.tool_policy.allowed {
        Some(allowed) => {
            allowed.contains("Skill")
                || (allowed.contains("list_skills") && allowed.contains("view_skill"))
                || allowed.contains("skill_hub")
                || allowed.contains("skill_config")
        }
        None => true,
    }
}

pub(crate) fn agent_catalog_for_policy(
    agent: &AgentDefinition,
    catalog: &[AgentDefinition],
) -> Vec<AgentDefinition> {
    if !agent_policy_allows_agent_catalog(agent) {
        return Vec::new();
    }
    catalog
        .iter()
        .filter(|candidate| candidate.supports_entrypoint(AgentEntrypoint::Subagent))
        .filter(|candidate| {
            agent
                .tool_policy
                .allowed_agents
                .as_ref()
                .is_none_or(|allowed| allowed.contains(&candidate.name))
        })
        .filter(|candidate| !agent.tool_policy.denied_agents.contains(&candidate.name))
        .cloned()
        .collect()
}

pub fn valid_agent_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_AGENT_NAME_LEN {
        return false;
    }
    let mut prev_dash = false;
    for ch in name.chars() {
        let valid = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-';
        if !valid {
            return false;
        }
        if ch == '-' {
            if prev_dash {
                return false;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
    }
    !name.starts_with('-') && !name.ends_with('-')
}

pub(crate) fn existing_agent_path(
    input: &str,
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Option<PathBuf>> {
    let raw = input.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let path = if raw == "~" {
        home_path(env)?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home_path(env)?.join(rest)
    } else {
        PathBuf::from(raw)
    };
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    Ok((path.is_file()).then_some(path))
}

pub(crate) fn ancestor_claude_agent_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut current = cwd.to_path_buf();
    loop {
        result.push(current.join(".claude").join("agents"));
        if current.join(".git").exists() {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
    }
    result
}

pub(crate) fn home_path(env: &BTreeMap<String, String>) -> Result<PathBuf> {
    env.get("HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::Config("HOME is required to expand ~".to_string()))
}

pub(crate) fn built_in_agents() -> Vec<AgentDefinition> {
    vec![
        built_in_agent(
            "general",
            "General-purpose subagent for focused coding tasks.",
            "You are a focused general-purpose Psychevo subagent. Work only on the assigned task and return a concise final summary.",
            None,
        ),
        built_in_agent(
            "plan-research",
            "Read-only planning and research subagent.",
            "You are a read-only planning subagent. Inspect context and produce a concrete plan. Use shell commands only for read-only exploration. Do not modify files or run mutating commands.",
            Some(
                ["read", "exec_command", "write_stdin"]
                    .into_iter()
                    .collect(),
            ),
        ),
        built_in_agent(
            "explore",
            "Read-only codebase exploration subagent.",
            "You are a read-only explorer. Answer specific codebase questions with file references, use shell commands only for read-only exploration, and avoid broad refactors.",
            Some(
                ["read", "exec_command", "write_stdin"]
                    .into_iter()
                    .collect(),
            ),
        ),
    ]
}

pub(crate) fn built_in_agent(
    name: &str,
    description: &str,
    instructions: &str,
    allowed: Option<BTreeSet<&str>>,
) -> AgentDefinition {
    AgentDefinition {
        name: name.to_string(),
        description: description.to_string(),
        instructions: instructions.to_string(),
        file_path: None,
        source: AgentSource::BuiltIn,
        backend: None,
        entrypoints: default_subagent_entrypoints(),
        model: None,
        tool_policy: AgentToolPolicy {
            allowed: allowed.map(|set| set.into_iter().map(str::to_string).collect()),
            denied: BTreeSet::new(),
            allowed_agents: None,
            denied_agents: BTreeSet::new(),
            permissions: None,
            permission_mode: None,
            mcp_servers: BTreeSet::new(),
        },
        skills: Vec::new(),
        hooks: None,
        background: None,
        initial_prompt: None,
        max_turns: None,
        max_spawn_depth: 0,
        project_instructions: None,
        effort: None,
        diagnostics: Vec::new(),
    }
}

pub(crate) struct SpawnAgentTool {
    pub(crate) context: AgentToolContext,
}

impl SpawnAgentTool {
    pub(crate) fn new(context: AgentToolContext) -> Self {
        Self { context }
    }
}

pub(crate) struct HookedTool {
    pub(crate) inner: Arc<dyn ToolBinding>,
    pub(crate) hook_runtime: crate::hooks::HookRuntime,
}
