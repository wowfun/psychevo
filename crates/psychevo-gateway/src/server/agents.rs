use super::*;

pub(super) fn read_agent_definition(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::AgentReadParams,
) -> psychevo_runtime::Result<Value> {
    let target = agent_config_target(params.target);
    let path = agent_definition_path(state, scope, target, &params.name)?;
    let text = std::fs::read_to_string(&path)?;
    let agent = parse_managed_agent_text(&text, &params.name, &path, target)?;
    Ok(serde_json::to_value(agent_read_result_with_raw(
        &agent, text,
    ))?)
}

pub(super) fn discover_gateway_teams(
    state: &WebState,
    scope: &ResolvedScope,
    agents: &AgentCatalog,
) -> psychevo_runtime::Result<AgentTeamCatalog> {
    discover_agent_teams_with_catalog(
        &AgentDiscoveryOptions {
            home: state.inner.home.clone(),
            cwd: scope.cwd.clone(),
            env: state.inner.inherited_env.clone(),
            explicit_inputs: Vec::new(),
            no_agents: false,
        },
        agents,
    )
}

pub(super) fn read_team_definition(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::TeamReadParams,
) -> psychevo_runtime::Result<Value> {
    let target = agent_config_target(params.target);
    let path = team_definition_path(state, scope, target, &params.name)?;
    let text = std::fs::read_to_string(&path)?;
    let agents = discover_gateway_agents(state, scope)?;
    let team = parse_managed_team_text(&text, &params.name, &path, target, &agents)?;
    Ok(serde_json::to_value(team_read_result_with_raw(
        &team, text,
    ))?)
}

pub(super) fn write_team_definition(
    state: &WebState,
    scope: &ResolvedScope,
    mut params: wire::TeamWriteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.name) {
        return Err(Error::Message(format!(
            "invalid team name: {}",
            params.name
        )));
    }
    let target = agent_config_target(params.target);
    let path = team_definition_path(state, scope, target, &params.name)?;
    let agents = discover_gateway_agents(state, scope)?;
    if params.raw_markdown.is_none() {
        let members = params
            .members
            .iter()
            .map(team_member_input)
            .collect::<psychevo_runtime::Result<Vec<_>>>()?;
        let captured = validate_and_capture_team_runtime_members(state, scope, &agents, &members)?;
        params.members = captured.iter().map(team_member_wire_input).collect();
    }
    let text = if let Some(raw_markdown) = params.raw_markdown.as_deref() {
        let team = parse_managed_team_text(raw_markdown, &params.name, &path, target, &agents)?;
        if team.name != params.name {
            return Err(Error::Message(format!(
                "raw team name `{}` must match requested name `{}`",
                team.name, params.name
            )));
        }
        raw_markdown.to_string()
    } else {
        structured_team_markdown(&path, &params)?
    };
    let team = parse_managed_team_text(&text, &params.name, &path, target, &agents)?;
    if team.name != params.name {
        return Err(Error::Message(format!(
            "team name `{}` must match requested name `{}`",
            team.name, params.name
        )));
    }
    // Raw Markdown is validated through the same authoritative seam. Missing
    // revisions can be captured later at Team activation; explicitly stale
    // revisions and unsafe pairings are rejected before the file is written.
    validate_and_capture_team_runtime_members(state, scope, &agents, &team.members)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &text)?;
    Ok(serde_json::to_value(wire::TeamWriteResult {
        written: true,
        name: params.name,
        path: path.display().to_string(),
        target,
        team: team_definition_view(&team),
    })?)
}

pub(super) fn set_team_definition_enabled(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::TeamSetEnabledParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.name) {
        return Err(Error::Message(format!(
            "invalid team name: {}",
            params.name
        )));
    }
    let target = agent_config_target(params.target);
    let path = team_definition_path(state, scope, target, &params.name)?;
    let existing = std::fs::read_to_string(&path)?;
    let (mut frontmatter, body) = split_agent_markdown(&existing)?;
    set_yaml_bool(&mut frontmatter, "enabled", params.enabled);
    let text = render_agent_markdown(frontmatter, &body)?;
    let agents = discover_gateway_agents(state, scope)?;
    let team = parse_managed_team_text(&text, &params.name, &path, target, &agents)?;
    validate_and_capture_team_runtime_members(state, scope, &agents, &team.members)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &text)?;
    Ok(serde_json::to_value(wire::TeamSetEnabledResult {
        written: true,
        name: params.name,
        path: path.display().to_string(),
        target,
        team: team_definition_view(&team),
    })?)
}

pub(super) fn delete_team_definition(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::TeamDeleteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.name) {
        return Err(Error::Message(format!(
            "invalid team name: {}",
            params.name
        )));
    }
    let target = agent_config_target(params.target);
    let path = team_definition_path(state, scope, target, &params.name)?;
    let deleted = match std::fs::remove_file(&path) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => return Err(err.into()),
    };
    Ok(serde_json::to_value(wire::TeamDeleteResult {
        deleted,
        name: params.name,
        path: path.display().to_string(),
        target,
    })?)
}

fn structured_team_markdown(
    path: &Path,
    params: &wire::TeamWriteParams,
) -> psychevo_runtime::Result<String> {
    let description = params.description.trim();
    if description.is_empty() {
        return Err(Error::Message(
            "team description must be non-empty".to_string(),
        ));
    }
    if !valid_agent_name(&params.leader) {
        return Err(Error::Message(format!(
            "invalid team leader: {}",
            params.leader
        )));
    }
    if params.members.is_empty() {
        return Err(Error::Message(
            "team members must include at least one member".to_string(),
        ));
    }
    let (mut frontmatter, _body) = match std::fs::read_to_string(path) {
        Ok(existing) => split_agent_markdown(&existing)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            (serde_yaml::Mapping::new(), String::new())
        }
        Err(err) => return Err(err.into()),
    };
    set_yaml_string(&mut frontmatter, "name", params.name.trim());
    set_yaml_string(&mut frontmatter, "description", description);
    set_yaml_bool(&mut frontmatter, "enabled", params.enabled.unwrap_or(true));
    set_yaml_string(&mut frontmatter, "leader", params.leader.trim());
    if let Some(max_parallel_agents) = params.max_parallel_agents {
        frontmatter.insert(
            serde_yaml::Value::String("maxParallelAgents".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(max_parallel_agents)),
        );
    } else {
        remove_yaml_key(&mut frontmatter, "maxParallelAgents");
    }
    let mut members = Vec::new();
    for member in &params.members {
        if !valid_agent_name(&member.id) {
            return Err(Error::Message(format!(
                "invalid team member id: {}",
                member.id
            )));
        }
        if !valid_agent_name(&member.agent) {
            return Err(Error::Message(format!(
                "invalid team member agent: {}",
                member.agent
            )));
        }
        let mut value = serde_yaml::Mapping::new();
        set_mapping_string(&mut value, "id", member.id.trim());
        set_mapping_string(&mut value, "agent", member.agent.trim());
        if let Some(runtime_ref) = member
            .runtime_ref
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            set_mapping_string(&mut value, "runtimeRef", runtime_ref);
        } else if !member.runtime_options.is_empty() {
            return Err(Error::Message(format!(
                "team member `{}` runtimeOptions require runtimeRef",
                member.id
            )));
        }
        if !member.runtime_options.is_empty() {
            let mut options = serde_yaml::Mapping::new();
            for (key, option) in &member.runtime_options {
                let key = key.trim();
                if key.is_empty() {
                    return Err(Error::Message(format!(
                        "team member `{}` runtimeOptions keys must be non-empty",
                        member.id
                    )));
                }
                set_mapping_string(&mut options, key, option);
            }
            value.insert(
                serde_yaml::Value::String("runtimeOptions".to_string()),
                serde_yaml::Value::Mapping(options),
            );
        }
        if let Some(revision) = member.runtime_profile_revision.as_deref() {
            let revision = revision.parse::<u64>().map_err(|_| {
                Error::Message(format!(
                    "team member `{}` runtimeProfileRevision must be an unsigned decimal string",
                    member.id
                ))
            })?;
            value.insert(
                serde_yaml::Value::String("runtimeProfileRevision".to_string()),
                serde_yaml::Value::Number(serde_yaml::Number::from(revision)),
            );
        }
        if let Some(role) = member
            .role
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            set_mapping_string(&mut value, "role", role);
        }
        if let Some(description) = member
            .description
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            set_mapping_string(&mut value, "description", description);
        }
        if let Some(max_turns) = member.max_turns {
            value.insert(
                serde_yaml::Value::String("maxTurns".to_string()),
                serde_yaml::Value::Number(serde_yaml::Number::from(max_turns as u64)),
            );
        }
        members.push(serde_yaml::Value::Mapping(value));
    }
    frontmatter.insert(
        serde_yaml::Value::String("members".to_string()),
        serde_yaml::Value::Sequence(members),
    );
    render_agent_markdown(frontmatter, params.instructions.trim())
}

fn team_member_input(member: &wire::TeamMemberInput) -> psychevo_runtime::Result<AgentTeamMember> {
    let runtime_profile_revision = member
        .runtime_profile_revision
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                Error::Message(format!(
                    "team member `{}` runtimeProfileRevision must be an unsigned decimal string",
                    member.id
                ))
            })
        })
        .transpose()?;
    Ok(AgentTeamMember {
        id: member.id.clone(),
        agent: member.agent.clone(),
        runtime_ref: member.runtime_ref.clone(),
        runtime_options: member.runtime_options.clone(),
        runtime_profile_revision,
        role: member.role.clone(),
        description: member.description.clone(),
        max_turns: member.max_turns,
    })
}

fn team_member_wire_input(member: &AgentTeamMember) -> wire::TeamMemberInput {
    wire::TeamMemberInput {
        id: member.id.clone(),
        agent: member.agent.clone(),
        runtime_ref: member.runtime_ref.clone(),
        runtime_options: member.runtime_options.clone(),
        runtime_profile_revision: member
            .runtime_profile_revision
            .map(|value| value.to_string()),
        role: member.role.clone(),
        description: member.description.clone(),
        max_turns: member.max_turns,
    }
}

pub(super) fn write_agent_definition(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::AgentWriteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.name) {
        return Err(Error::Message(format!(
            "invalid agent name: {}",
            params.name
        )));
    }
    let target = agent_config_target(params.target);
    let path = agent_definition_path(state, scope, target, &params.name)?;
    let text = if let Some(raw_markdown) = params.raw_markdown.as_deref() {
        let agent = parse_managed_agent_text(raw_markdown, &params.name, &path, target)?;
        if agent.name != params.name {
            return Err(Error::Message(format!(
                "raw agent name `{}` must match requested name `{}`",
                agent.name, params.name
            )));
        }
        raw_markdown.to_string()
    } else {
        structured_agent_markdown(&path, &params)?
    };
    let agent = parse_managed_agent_text(&text, &params.name, &path, target)?;
    if agent.name != params.name {
        return Err(Error::Message(format!(
            "agent name `{}` must match requested name `{}`",
            agent.name, params.name
        )));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &text)?;
    Ok(serde_json::to_value(wire::AgentWriteResult {
        written: true,
        name: params.name,
        path: path.display().to_string(),
        target,
        agent: agent_definition_view(&agent),
    })?)
}

pub(super) fn set_agent_definition_enabled(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::AgentSetEnabledParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.name) {
        return Err(Error::Message(format!(
            "invalid agent name: {}",
            params.name
        )));
    }
    let target = agent_config_target(params.target);
    let path = agent_definition_path(state, scope, target, &params.name)?;
    let existing = std::fs::read_to_string(&path)?;
    let (mut frontmatter, body) = split_agent_markdown(&existing)?;
    set_yaml_bool(&mut frontmatter, "enabled", params.enabled);
    let text = render_agent_markdown(frontmatter, &body)?;
    let agent = parse_managed_agent_text(&text, &params.name, &path, target)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &text)?;
    Ok(serde_json::to_value(wire::AgentSetEnabledResult {
        written: true,
        name: params.name,
        path: path.display().to_string(),
        target,
        agent: agent_definition_view(&agent),
    })?)
}

fn structured_agent_markdown(
    path: &Path,
    params: &wire::AgentWriteParams,
) -> psychevo_runtime::Result<String> {
    let description = params.description.trim();
    if description.is_empty() {
        return Err(Error::Message(
            "agent description must be non-empty".to_string(),
        ));
    }
    if let Some(backend) = &params.backend
        && !valid_agent_name(&backend.name)
    {
        return Err(Error::Message(format!(
            "invalid backend ref: {}",
            backend.name
        )));
    }
    let mut entrypoints = Vec::new();
    for entrypoint in &params.entrypoints {
        let parsed = AgentEntrypoint::parse(entrypoint).ok_or_else(|| {
            Error::Message(format!(
                "agent entrypoint `{entrypoint}` must be peer or subagent"
            ))
        })?;
        entrypoints.push(parsed.as_str().to_string());
    }
    let (mut frontmatter, _body) = match std::fs::read_to_string(path) {
        Ok(existing) => split_agent_markdown(&existing)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            (serde_yaml::Mapping::new(), String::new())
        }
        Err(err) => return Err(err.into()),
    };
    set_yaml_string(&mut frontmatter, "name", params.name.trim());
    set_yaml_string(&mut frontmatter, "description", description);
    set_yaml_bool(&mut frontmatter, "enabled", params.enabled.unwrap_or(true));
    if let Some(backend) = params.backend.as_ref() {
        let mut backend_value = serde_yaml::Mapping::new();
        backend_value.insert(
            serde_yaml::Value::String("ref".to_string()),
            serde_yaml::Value::String(backend.name.clone()),
        );
        frontmatter.insert(
            serde_yaml::Value::String("backend".to_string()),
            serde_yaml::Value::Mapping(backend_value),
        );
    } else {
        remove_yaml_key(&mut frontmatter, "backend");
    }
    if !entrypoints.is_empty() {
        frontmatter.insert(
            serde_yaml::Value::String("entrypoints".to_string()),
            serde_yaml::Value::Sequence(
                entrypoints
                    .into_iter()
                    .map(serde_yaml::Value::String)
                    .collect(),
            ),
        );
    } else {
        remove_yaml_key(&mut frontmatter, "entrypoints");
    }
    if !params.tools.is_empty() {
        frontmatter.insert(
            serde_yaml::Value::String("tools".to_string()),
            serde_yaml::Value::Sequence(
                params
                    .tools
                    .iter()
                    .filter(|tool| !tool.trim().is_empty())
                    .map(|tool| serde_yaml::Value::String(tool.trim().to_string()))
                    .collect(),
            ),
        );
    } else {
        remove_yaml_key(&mut frontmatter, "tools");
    }
    if !params.mcp_servers.is_empty() {
        frontmatter.insert(
            serde_yaml::Value::String("mcpServers".to_string()),
            serde_yaml::Value::Sequence(
                params
                    .mcp_servers
                    .iter()
                    .filter(|server| !server.trim().is_empty())
                    .map(|server| serde_yaml::Value::String(server.trim().to_string()))
                    .collect(),
            ),
        );
    } else {
        remove_yaml_key(&mut frontmatter, "mcpServers");
    }
    let mut optional_contributions = BTreeSet::new();
    for name in &params.optional_contributions {
        let contribution = psychevo_runtime::AgentContribution::parse(name).ok_or_else(|| {
            Error::Message(format!(
                "optional contribution `{name}` must be instructions, tools, mcp, or skills"
            ))
        })?;
        optional_contributions.insert(contribution.as_str());
    }
    if optional_contributions.is_empty() {
        remove_yaml_key(&mut frontmatter, "optionalContributions");
    } else {
        frontmatter.insert(
            serde_yaml::Value::String("optionalContributions".to_string()),
            serde_yaml::Value::Sequence(
                optional_contributions
                    .into_iter()
                    .map(|name| serde_yaml::Value::String(name.to_string()))
                    .collect(),
            ),
        );
    }
    render_agent_markdown(frontmatter, params.instructions.trim())
}

pub(super) fn delete_agent_definition(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::AgentDeleteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.name) {
        return Err(Error::Message(format!(
            "invalid agent name: {}",
            params.name
        )));
    }
    let target = agent_config_target(params.target);
    let path = agent_definition_path(state, scope, target, &params.name)?;
    let deleted = match std::fs::remove_file(&path) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => return Err(err.into()),
    };
    Ok(serde_json::to_value(wire::AgentDeleteResult {
        deleted,
        name: params.name,
        path: path.display().to_string(),
        target,
    })?)
}

fn agent_config_target(target: Option<wire::AgentConfigTarget>) -> wire::AgentConfigTarget {
    target.unwrap_or(wire::AgentConfigTarget::Project)
}

fn agent_definition_path(
    state: &WebState,
    scope: &ResolvedScope,
    target: wire::AgentConfigTarget,
    name: &str,
) -> psychevo_runtime::Result<PathBuf> {
    if !valid_agent_name(name) {
        return Err(Error::Message(format!("invalid agent name: {name}")));
    }
    let root = match target {
        wire::AgentConfigTarget::Project => scope.cwd.join(".psychevo"),
        wire::AgentConfigTarget::Profile => active_profile_config_dir(state, scope),
    };
    Ok(root.join("agents").join(format!("{name}.md")))
}

fn team_definition_path(
    state: &WebState,
    scope: &ResolvedScope,
    target: wire::AgentConfigTarget,
    name: &str,
) -> psychevo_runtime::Result<PathBuf> {
    if !valid_agent_name(name) {
        return Err(Error::Message(format!("invalid team name: {name}")));
    }
    let root = match target {
        wire::AgentConfigTarget::Project => scope.cwd.join(".psychevo"),
        wire::AgentConfigTarget::Profile => active_profile_config_dir(state, scope),
    };
    Ok(root.join("teams").join(format!("{name}.md")))
}

fn agent_source_for_target(target: wire::AgentConfigTarget) -> AgentSource {
    match target {
        wire::AgentConfigTarget::Project => AgentSource::Project,
        wire::AgentConfigTarget::Profile => AgentSource::Global,
    }
}

fn team_source_for_target(target: wire::AgentConfigTarget) -> AgentTeamSource {
    match target {
        wire::AgentConfigTarget::Project => AgentTeamSource::Project,
        wire::AgentConfigTarget::Profile => AgentTeamSource::Profile,
    }
}

fn parse_managed_agent_text(
    text: &str,
    name: &str,
    path: &Path,
    target: wire::AgentConfigTarget,
) -> psychevo_runtime::Result<AgentDefinition> {
    let agent = parse_agent_definition_text(
        text,
        name,
        Some(path.to_path_buf()),
        agent_source_for_target(target),
    )?;
    if !valid_agent_name(&agent.name) {
        return Err(Error::Message(format!(
            "invalid agent name: {}",
            agent.name
        )));
    }
    Ok(agent)
}

fn parse_managed_team_text(
    text: &str,
    name: &str,
    path: &Path,
    target: wire::AgentConfigTarget,
    agents: &AgentCatalog,
) -> psychevo_runtime::Result<AgentTeamDefinition> {
    let team = parse_agent_team_definition_text(
        text,
        name,
        Some(path.to_path_buf()),
        team_source_for_target(target),
        agents,
    )?;
    if !valid_agent_name(&team.name) {
        return Err(Error::Message(format!("invalid team name: {}", team.name)));
    }
    Ok(team)
}

fn split_agent_markdown(content: &str) -> psychevo_runtime::Result<(serde_yaml::Mapping, String)> {
    let Some(rest) = content.strip_prefix("---\n") else {
        return Ok((serde_yaml::Mapping::new(), content.to_string()));
    };
    let Some(end) = rest.find("\n---") else {
        return Err(Error::Config("agent frontmatter is not closed".to_string()));
    };
    let frontmatter = &rest[..end];
    let body = rest[end + "\n---".len()..]
        .strip_prefix('\n')
        .unwrap_or(&rest[end + "\n---".len()..])
        .to_string();
    let parsed = serde_yaml::from_str::<serde_yaml::Value>(frontmatter)?;
    let mapping = match parsed {
        serde_yaml::Value::Mapping(mapping) => mapping,
        serde_yaml::Value::Null => serde_yaml::Mapping::new(),
        _ => {
            return Err(Error::Config(
                "agent frontmatter must be a YAML mapping".to_string(),
            ));
        }
    };
    Ok((mapping, body))
}

fn render_agent_markdown(
    frontmatter: serde_yaml::Mapping,
    body: &str,
) -> psychevo_runtime::Result<String> {
    let frontmatter = serde_yaml::to_string(&frontmatter)?;
    let body = body.trim();
    Ok(if body.is_empty() {
        format!("---\n{frontmatter}---\n")
    } else {
        format!("---\n{frontmatter}---\n{body}\n")
    })
}

fn yaml_key(key: &str) -> serde_yaml::Value {
    serde_yaml::Value::String(key.to_string())
}

fn remove_yaml_key(frontmatter: &mut serde_yaml::Mapping, key: &str) {
    frontmatter.remove(yaml_key(key));
}

fn set_yaml_string(frontmatter: &mut serde_yaml::Mapping, key: &str, value: &str) {
    frontmatter.insert(yaml_key(key), serde_yaml::Value::String(value.to_string()));
}

fn set_mapping_string(mapping: &mut serde_yaml::Mapping, key: &str, value: &str) {
    mapping.insert(yaml_key(key), serde_yaml::Value::String(value.to_string()));
}

fn set_yaml_bool(frontmatter: &mut serde_yaml::Mapping, key: &str, value: bool) {
    frontmatter.insert(yaml_key(key), serde_yaml::Value::Bool(value));
}

pub(super) fn agent_list_result(catalog: &AgentCatalog) -> wire::AgentListResult {
    wire::AgentListResult {
        agents: catalog.agents.iter().map(agent_definition_view).collect(),
        shadowed_agents: catalog
            .shadowed_agents
            .iter()
            .map(agent_definition_view)
            .collect(),
        disabled_agents: catalog
            .disabled_agents
            .iter()
            .map(agent_definition_view)
            .collect(),
        diagnostics: catalog
            .diagnostics
            .iter()
            .map(agent_diagnostic_view)
            .collect(),
    }
}

pub(super) fn team_list_result(catalog: &AgentTeamCatalog) -> wire::TeamListResult {
    wire::TeamListResult {
        teams: catalog.teams.iter().map(team_definition_view).collect(),
        shadowed_teams: catalog
            .shadowed_teams
            .iter()
            .map(team_definition_view)
            .collect(),
        disabled_teams: catalog
            .disabled_teams
            .iter()
            .map(team_definition_view)
            .collect(),
        diagnostics: catalog
            .diagnostics
            .iter()
            .map(agent_diagnostic_view)
            .collect(),
    }
}

pub(super) fn agent_read_result(agent: &AgentDefinition) -> wire::AgentReadResult {
    let raw_markdown = agent
        .file_path
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    agent_read_result_with_raw(agent, raw_markdown)
}

fn agent_read_result_with_raw(
    agent: &AgentDefinition,
    raw_markdown: String,
) -> wire::AgentReadResult {
    wire::AgentReadResult {
        agent: agent_definition_view(agent),
        instructions: agent.instructions.clone(),
        raw_markdown,
    }
}

pub(super) fn team_read_result(team: &AgentTeamDefinition) -> wire::TeamReadResult {
    let raw_markdown = team
        .file_path
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    team_read_result_with_raw(team, raw_markdown)
}

fn team_read_result_with_raw(
    team: &AgentTeamDefinition,
    raw_markdown: String,
) -> wire::TeamReadResult {
    wire::TeamReadResult {
        team: team_definition_view(team),
        instructions: team.instructions.clone(),
        raw_markdown,
    }
}

fn agent_definition_view(agent: &AgentDefinition) -> wire::AgentDefinitionView {
    let target = agent_definition_target(agent);
    let tools = agent
        .tool_policy
        .allowed
        .as_ref()
        .map(|tools| tools.iter().cloned().collect())
        .unwrap_or_default();
    let mut contributions = Vec::new();
    if !agent.instructions.trim().is_empty() {
        contributions.push(wire::AgentContributionView::Instructions);
    }
    if agent.tool_policy.allowed.is_some()
        || !agent.tool_policy.denied.is_empty()
        || agent.tool_policy.allowed_agents.is_some()
        || !agent.tool_policy.denied_agents.is_empty()
    {
        contributions.push(wire::AgentContributionView::Tools);
    }
    if !agent.tool_policy.mcp_servers.is_empty() {
        contributions.push(wire::AgentContributionView::Mcp);
    }
    if !agent.skills.is_empty() {
        contributions.push(wire::AgentContributionView::Skills);
    }
    wire::AgentDefinitionView {
        name: agent.name.clone(),
        description: agent.description.clone(),
        enabled: agent.enabled,
        source: agent.source.as_str().to_string(),
        source_label: agent.source.display_label().to_string(),
        generated: matches!(agent.source, psychevo_runtime::AgentSource::Generated),
        target,
        mutable: target.is_some(),
        path: agent
            .file_path
            .as_ref()
            .map(|path| path.display().to_string()),
        backend: agent
            .backend
            .as_ref()
            .map(|backend| wire::AgentBackendRefView {
                name: backend.name.clone(),
            }),
        entrypoints: agent
            .entrypoints
            .iter()
            .map(|entrypoint| entrypoint.as_str().to_string())
            .collect(),
        tools,
        mcp_servers: agent.tool_policy.mcp_servers.iter().cloned().collect(),
        contributions,
        optional_contributions: agent
            .optional_contributions
            .iter()
            .map(|contribution| contribution.as_str().to_string())
            .collect(),
        diagnostics: agent
            .diagnostics
            .iter()
            .map(agent_diagnostic_view)
            .collect(),
    }
}

fn team_definition_view(team: &AgentTeamDefinition) -> wire::TeamDefinitionView {
    let target = team_definition_target(team);
    wire::TeamDefinitionView {
        name: team.name.clone(),
        description: team.description.clone(),
        enabled: team.enabled,
        source: team.source.as_str().to_string(),
        source_label: team.source.display_label().to_string(),
        target,
        mutable: target.is_some(),
        path: team
            .file_path
            .as_ref()
            .map(|path| path.display().to_string()),
        leader: team.leader.clone(),
        members: team.members.iter().map(team_member_view).collect(),
        max_parallel_agents: team.max_parallel_agents,
        diagnostics: team.diagnostics.iter().map(agent_diagnostic_view).collect(),
    }
}

fn agent_definition_target(agent: &AgentDefinition) -> Option<wire::AgentConfigTarget> {
    match agent.source {
        AgentSource::Project => Some(wire::AgentConfigTarget::Project),
        AgentSource::Global => Some(wire::AgentConfigTarget::Profile),
        _ => None,
    }
}

fn team_definition_target(team: &AgentTeamDefinition) -> Option<wire::AgentConfigTarget> {
    match team.source {
        AgentTeamSource::Project => Some(wire::AgentConfigTarget::Project),
        AgentTeamSource::Profile => Some(wire::AgentConfigTarget::Profile),
    }
}

fn team_member_view(member: &AgentTeamMember) -> wire::TeamMemberView {
    wire::TeamMemberView {
        id: member.id.clone(),
        agent: member.agent.clone(),
        runtime_ref: member.runtime_ref.clone(),
        runtime_options: member.runtime_options.clone(),
        runtime_profile_revision: member
            .runtime_profile_revision
            .map(|value| value.to_string()),
        role: member.role.clone(),
        description: member.description.clone(),
        max_turns: member.max_turns,
    }
}

fn agent_diagnostic_view(diagnostic: &AgentDiagnostic) -> wire::AgentDiagnosticView {
    wire::AgentDiagnosticView {
        kind: diagnostic.kind.clone(),
        message: diagnostic.message.clone(),
        path: diagnostic
            .path
            .as_ref()
            .map(|path| path.display().to_string()),
    }
}

pub(super) fn agent_status_result(
    store: Option<&psychevo_runtime::SqliteStore>,
    parent_session_id: Option<&str>,
    all: bool,
) -> wire::AgentStatusResult {
    wire::AgentStatusResult {
        agents: agent_status_records(store, parent_session_id, all)
            .iter()
            .map(agent_run_view)
            .collect(),
        control: agent_status_control_view(),
    }
}

pub(super) fn team_status_result(
    store: &psychevo_runtime::SqliteStore,
    parent_session_id: Option<&str>,
) -> psychevo_runtime::Result<wire::TeamStatusResult> {
    let team = parent_session_id
        .map(|thread| store.find_active_agent_team_run(thread))
        .transpose()?
        .flatten()
        .or_else(|| {
            parent_session_id.and_then(|thread| {
                store
                    .list_agent_team_runs_for_parent(thread)
                    .ok()
                    .and_then(|runs| runs.into_iter().next())
            })
        });
    let mission = parent_session_id
        .map(|thread| store.find_active_agent_mission_run(thread))
        .transpose()?
        .flatten()
        .or_else(|| {
            parent_session_id.and_then(|thread| {
                store
                    .list_agent_mission_runs_for_parent(thread)
                    .ok()
                    .and_then(|runs| runs.into_iter().next())
            })
        });
    Ok(wire::TeamStatusResult {
        team: team.as_ref().map(team_run_view),
        mission: mission.as_ref().map(mission_run_view),
        agents: agent_status_records(Some(store), parent_session_id, false)
            .iter()
            .map(agent_run_view)
            .collect(),
        control: agent_status_control_view(),
    })
}

pub(super) fn agent_control_result(
    store: &psychevo_runtime::SqliteStore,
    params: wire::AgentControlParams,
) -> psychevo_runtime::Result<wire::AgentControlResult> {
    let action = params.action.trim();
    let agent = match action {
        "stop" => {
            let target = required_control_target(&params)?;
            stop_agent_id_with_grace(target, Some(store), std::time::Duration::from_millis(250))?
        }
        "resume" => {
            let target = required_control_target(&params)?;
            resume_agent_id(target, Some(store))?
        }
        "send" => {
            let target = required_control_target(&params)?;
            let message = params
                .message
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| Error::Message("agent/control send requires message".to_string()))?;
            send_agent_message(target, message, Some(store))?
        }
        "pauseSpawning" => {
            set_agent_spawn_paused(true);
            None
        }
        "resumeSpawning" => {
            set_agent_spawn_paused(false);
            None
        }
        other => {
            return Err(Error::Message(format!(
                "unsupported agent/control action: {other}"
            )));
        }
    };
    Ok(wire::AgentControlResult {
        accepted: matches!(action, "pauseSpawning" | "resumeSpawning") || agent.is_some(),
        agent: agent.as_ref().map(agent_run_view),
        control: agent_status_control_view(),
    })
}

fn required_control_target(params: &wire::AgentControlParams) -> psychevo_runtime::Result<&str> {
    params
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Message("agent/control action requires target".to_string()))
}

fn agent_status_control_view() -> wire::AgentStatusControlView {
    wire::AgentStatusControlView {
        spawning_paused: agent_spawn_paused(),
        max_spawn_depth_cap: MAX_AGENT_SPAWN_DEPTH_CAP,
        concurrency_cap: Some(MAX_TEAM_PARALLEL_AGENTS_CAP),
    }
}

fn agent_run_view(record: &AgentRunRecord) -> wire::AgentRunView {
    wire::AgentRunView {
        id: record.id.clone(),
        task_name: record.task_name.clone(),
        agent_name: record.agent_name.clone(),
        task: record.task.clone(),
        parent_session_id: record.parent_session_id.clone(),
        child_session_id: record.child_session_id.clone(),
        role: record.role.as_str().to_string(),
        background: record.background,
        status: record.status.as_str().to_string(),
        edge_status: record.edge_status.map(|status| status.as_str().to_string()),
        started_at_ms: record.started_at_ms,
        ended_at_ms: record.ended_at_ms,
        outcome: record.outcome.clone(),
        final_answer: record.final_answer.clone(),
        error: record.error.clone(),
        effective_max_spawn_depth: record.effective_max_spawn_depth,
        team_run_id: record.team_run_id.clone(),
        mission_run_id: record.mission_run_id.clone(),
        team_name: record.team_name.clone(),
        team_member_id: record.team_member_id.clone(),
        agent_path: record.agent_path.clone(),
    }
}

fn team_run_view(record: &psychevo_runtime::AgentTeamRunRecord) -> wire::TeamRunView {
    wire::TeamRunView {
        id: record.id.clone(),
        parent_session_id: record.parent_session_id.clone(),
        mission_run_id: record.mission_run_id.clone(),
        team_name: record.team_name.clone(),
        description: record.description.clone(),
        source_path: record.source_path.clone(),
        leader_agent_name: record.leader_agent_name.clone(),
        members: serde_json::from_value::<Vec<AgentTeamMember>>(record.members.clone())
            .unwrap_or_default()
            .iter()
            .map(team_member_view)
            .collect(),
        max_parallel_agents: record.max_parallel_agents,
        status: record.status.clone(),
        started_at_ms: record.started_at_ms,
        ended_at_ms: record.ended_at_ms,
        final_summary: record.final_summary.clone(),
    }
}

fn mission_run_view(record: &psychevo_runtime::AgentMissionRunRecord) -> wire::MissionRunView {
    wire::MissionRunView {
        id: record.id.clone(),
        parent_session_id: record.parent_session_id.clone(),
        team_run_id: record.team_run_id.clone(),
        team_name: record.team_name.clone(),
        goal: record.goal.clone(),
        lead_agent_name: record.lead_agent_name.clone(),
        status: record.status.clone(),
        started_at_ms: record.started_at_ms,
        ended_at_ms: record.ended_at_ms,
        final_summary: record.final_summary.clone(),
    }
}

fn backend_value_with_sources(
    backend: &AgentBackendConfig,
    source_targets: Vec<wire::BackendConfigTarget>,
) -> wire::BackendConfigView {
    wire::BackendConfigView {
        id: backend.id.clone(),
        kind: backend.kind.as_str().to_string(),
        enabled: backend.enabled,
        label: backend.label.clone(),
        description: backend.description.clone(),
        command: backend.command.clone(),
        args: backend.args.clone(),
        cwd: backend.cwd.clone(),
        entrypoints: backend
            .entrypoints
            .iter()
            .map(|entrypoint| entrypoint.as_str().to_string())
            .collect(),
        client_capabilities: backend.client_capabilities.iter().cloned().collect(),
        mcp_servers: backend.mcp_servers.iter().cloned().collect(),
        env_keys: backend.env.keys().cloned().collect(),
        source_targets,
        diagnostics: backend_diagnostics(backend),
    }
}

pub(super) fn backend_values_for_scope(
    state: &WebState,
    scope: &ResolvedScope,
    backends: &BTreeMap<String, AgentBackendConfig>,
) -> psychevo_runtime::Result<Vec<wire::BackendConfigView>> {
    backends
        .values()
        .map(|backend| {
            Ok(backend_value_with_sources(
                backend,
                backend_source_targets(state, scope, &backend.id)?,
            ))
        })
        .collect()
}

pub(super) fn materialize_local_acp_backends(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<()> {
    let existing_backends =
        load_agent_backend_configs(&state.inner.home, &scope.cwd, &state.inner.inherited_env)?;
    let config_dir = active_profile_config_dir(state, scope);
    if !existing_backends.contains_key(crate::managed_acp::CODEX_ACP_BACKEND_ID) {
        let paths =
            crate::managed_acp::managed_codex_acp_paths(&state.inner.home, HostPlatform::current());
        set_config_value(
            config_dir.clone(),
            "agents.backends.codex",
            managed_codex_acp_backend_config_json(&paths.executable),
        )?;
    }
    for shortcut in local_acp_backend_shortcuts() {
        if existing_backends.contains_key(shortcut.id) {
            continue;
        }
        if resolve_command_path(
            shortcut.command,
            &state.inner.inherited_env,
            &scope.cwd,
            HostPlatform::current(),
        )
        .is_none()
        {
            continue;
        }
        set_config_value(
            config_dir.clone(),
            &format!("agents.backends.{}", shortcut.id),
            local_acp_backend_config_json(shortcut),
        )?;
    }
    Ok(())
}

fn managed_codex_acp_backend_config_json(command: &Path) -> Value {
    json!({
        "kind": "acp",
        "enabled": true,
        "label": "Codex",
        "description": "Codex through the managed ACP adapter.",
        "command": command.display().to_string(),
        "args": [],
        "env": {},
        "cwd": "invocation",
        "entrypoints": ["peer", "subagent"],
        "client_capabilities": ["fs.read", "fs.write", "terminal"],
        "mcp_servers": []
    })
}

struct LocalAcpBackendShortcut {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    command: &'static str,
    args: &'static [&'static str],
}

fn local_acp_backend_shortcuts() -> &'static [LocalAcpBackendShortcut] {
    &[
        LocalAcpBackendShortcut {
            id: "opencode",
            label: "OpenCode",
            description: "OpenCode ACP coding agent.",
            command: "opencode",
            args: &["acp"],
        },
        LocalAcpBackendShortcut {
            id: "hermes",
            label: "Hermes",
            description: "Hermes ACP coding agent.",
            command: "hermes",
            args: &["acp"],
        },
    ]
}

fn local_acp_backend_config_json(shortcut: &LocalAcpBackendShortcut) -> Value {
    json!({
        "kind": "acp",
        "enabled": true,
        "label": shortcut.label,
        "description": shortcut.description,
        "command": shortcut.command,
        "args": shortcut.args,
        "env": {},
        "cwd": "invocation",
        "entrypoints": ["peer", "subagent"],
        "client_capabilities": ["fs.read", "fs.write", "terminal"],
        "mcp_servers": []
    })
}

pub(super) fn write_backend_config(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::BackendWriteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.id) {
        return Err(Error::Message(format!("invalid backend id: {}", params.id)));
    }
    ensure_profile_config_for_backend_write(state, scope, params.target)?;
    let existing_backends =
        load_agent_backend_configs(&state.inner.home, &scope.cwd, &state.inner.inherited_env)?;
    let value = backend_config_json(&params, existing_backends.get(&params.id))?;
    let target = params.target;
    let config_dir = backend_config_dir(state, scope, target)?;
    let result = set_config_value(config_dir, &format!("agents.backends.{}", params.id), value)?;
    let backends =
        load_agent_backend_configs(&state.inner.home, &scope.cwd, &state.inner.inherited_env)?;
    let backend = backends
        .get(&params.id)
        .ok_or_else(|| Error::Message(format!("backend write did not reload: {}", params.id)))?;
    Ok(serde_json::to_value(wire::BackendWriteResult {
        written: true,
        changed: result.changed,
        path: result.path.display().to_string(),
        target,
        backend: backend_value_with_sources(
            backend,
            backend_source_targets(state, scope, &backend.id)?,
        ),
    })?)
}

pub(super) fn delete_backend_config(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::BackendDeleteParams,
) -> psychevo_runtime::Result<Value> {
    if !valid_agent_name(&params.id) {
        return Err(Error::Message(format!("invalid backend id: {}", params.id)));
    }
    let target = params.target;
    let config_dir = backend_config_dir(state, scope, target)?;
    let result = remove_config_value(config_dir, &format!("agents.backends.{}", params.id))?;
    Ok(serde_json::to_value(wire::BackendDeleteResult {
        deleted: result.changed,
        changed: result.changed,
        id: params.id,
        path: result.path.display().to_string(),
        target,
    })?)
}

fn backend_config_dir(
    state: &WebState,
    scope: &ResolvedScope,
    target: wire::BackendConfigTarget,
) -> psychevo_runtime::Result<PathBuf> {
    match target {
        wire::BackendConfigTarget::Project => Ok(scope.cwd.join(".psychevo")),
        wire::BackendConfigTarget::Profile => Ok(active_profile_config_dir(state, scope)),
    }
}

fn ensure_profile_config_for_backend_write(
    state: &WebState,
    scope: &ResolvedScope,
    target: wire::BackendConfigTarget,
) -> psychevo_runtime::Result<()> {
    if target != wire::BackendConfigTarget::Profile
        || !state
            .inner
            .inherited_env
            .get("PSYCHEVO_CONFIG")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    {
        return Ok(());
    }
    let config_path = active_profile_config_dir(state, scope).join("config.toml");
    if config_path.exists() {
        return Ok(());
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(config_path, "")?;
    Ok(())
}

pub(super) fn active_profile_config_dir(state: &WebState, scope: &ResolvedScope) -> PathBuf {
    state
        .inner
        .inherited_env
        .get("PSYCHEVO_CONFIG")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .and_then(|value| {
            let path = resolve_gateway_env_path(value, state, scope);
            path.parent().map(Path::to_path_buf)
        })
        .unwrap_or_else(|| state.inner.home.clone())
}

fn resolve_gateway_env_path(value: &str, state: &WebState, scope: &ResolvedScope) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        let home = state
            .inner
            .inherited_env
            .get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| state.inner.home.clone());
        return home.join(rest);
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        scope.cwd.join(path)
    }
}

fn backend_config_json(
    params: &wire::BackendWriteParams,
    existing: Option<&AgentBackendConfig>,
) -> psychevo_runtime::Result<Value> {
    let entrypoints = if params.entrypoints.is_empty() {
        vec!["peer".to_string(), "subagent".to_string()]
    } else {
        validate_backend_entrypoints(&params.entrypoints)?
    };
    let client_capabilities = if params.client_capabilities.is_empty() {
        vec![
            "fs.read".to_string(),
            "fs.write".to_string(),
            "terminal".to_string(),
        ]
    } else {
        validate_backend_client_capabilities(&params.client_capabilities)?
    };
    let args = trimmed_string_list(&params.args);
    let mcp_servers = trimmed_string_list(&params.mcp_servers);
    let env = if params.env.is_empty() {
        existing
            .map(|backend| backend.env.clone())
            .unwrap_or_default()
    } else {
        params
            .env
            .iter()
            .filter_map(|(key, value)| {
                let key = key.trim();
                if key.is_empty() {
                    None
                } else {
                    Some((key.to_string(), value.to_string()))
                }
            })
            .collect::<BTreeMap<_, _>>()
    };
    let label = params
        .label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cwd = params
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("invocation");
    let mut object = serde_json::Map::new();
    object.insert("kind".to_string(), json!("acp"));
    object.insert("enabled".to_string(), json!(params.enabled.unwrap_or(true)));
    if let Some(label) = label {
        object.insert("label".to_string(), json!(label));
    }
    if let Some(description) = params
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("description".to_string(), json!(description));
    }
    if let Some(command) = params
        .command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert("command".to_string(), json!(command));
    }
    object.insert("args".to_string(), json!(args));
    object.insert("env".to_string(), json!(env));
    object.insert("cwd".to_string(), json!(cwd));
    object.insert("entrypoints".to_string(), json!(entrypoints));
    object.insert(
        "client_capabilities".to_string(),
        json!(client_capabilities),
    );
    object.insert("mcp_servers".to_string(), json!(mcp_servers));
    Ok(Value::Object(object))
}

fn validate_backend_entrypoints(values: &[String]) -> psychevo_runtime::Result<Vec<String>> {
    let mut entrypoints = Vec::new();
    for value in values {
        let value = value.trim();
        let entrypoint = AgentEntrypoint::parse(value).ok_or_else(|| {
            Error::Message(format!(
                "backend entrypoint `{value}` must be peer or subagent"
            ))
        })?;
        let entrypoint = entrypoint.as_str().to_string();
        if !entrypoints.contains(&entrypoint) {
            entrypoints.push(entrypoint);
        }
    }
    if entrypoints.is_empty() {
        return Err(Error::Message(
            "backend entrypoints must include peer or subagent".to_string(),
        ));
    }
    Ok(entrypoints)
}

fn validate_backend_client_capabilities(
    values: &[String],
) -> psychevo_runtime::Result<Vec<String>> {
    let mut capabilities = Vec::new();
    for value in values {
        let value = value.trim();
        if !matches!(value, "fs.read" | "fs.write" | "terminal") {
            return Err(Error::Message(format!(
                "backend client capability `{value}` must be fs.read, fs.write, or terminal"
            )));
        }
        if !capabilities.iter().any(|capability| capability == value) {
            capabilities.push(value.to_string());
        }
    }
    Ok(capabilities)
}

fn trimmed_string_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn backend_source_targets(
    state: &WebState,
    scope: &ResolvedScope,
    id: &str,
) -> psychevo_runtime::Result<Vec<wire::BackendConfigTarget>> {
    let mut targets = Vec::new();
    if backend_exists_in_scope(state, scope, id, ConfigScope::Global)? {
        targets.push(wire::BackendConfigTarget::Profile);
    }
    if backend_exists_in_scope(state, scope, id, ConfigScope::Local)? {
        targets.push(wire::BackendConfigTarget::Project);
    }
    Ok(targets)
}

fn backend_exists_in_scope(
    state: &WebState,
    scope: &ResolvedScope,
    id: &str,
    config_scope: ConfigScope,
) -> psychevo_runtime::Result<bool> {
    let config_dir = match config_scope {
        ConfigScope::Global => active_profile_config_dir(state, scope),
        ConfigScope::Local => scope.cwd.join(".psychevo"),
        ConfigScope::Effective => {
            return Err(Error::Config(
                "backend source target checks require a concrete config scope".to_string(),
            ));
        }
    };
    backend_exists_in_config_dir(&config_dir, id)
}

fn backend_exists_in_config_dir(config_dir: &Path, id: &str) -> psychevo_runtime::Result<bool> {
    let config_path = config_dir.join("config.toml");
    if !config_path.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&config_path)?;
    let parsed: toml::Value = toml::from_str(&text)
        .map_err(|err| Error::Config(format!("{}: {err}", config_path.display())))?;
    Ok(parsed
        .get("agents")
        .and_then(|value| value.get("backends"))
        .and_then(|value| value.get(id))
        .is_some())
}

fn backend_diagnostics(backend: &AgentBackendConfig) -> Vec<wire::BackendDiagnosticView> {
    let mut diagnostics = Vec::new();
    if !backend.enabled {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "disabled".to_string(),
            message: "backend is disabled".to_string(),
        });
    }
    if backend.command.is_none() {
        diagnostics.push(wire::BackendDiagnosticView {
            kind: "missing_command".to_string(),
            message: "backend command is required for execution".to_string(),
        });
    }
    diagnostics
}

pub(super) async fn manage_backend_value(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::BackendManageParams,
    operation: &str,
) -> psychevo_runtime::Result<Value> {
    if params.id != crate::managed_acp::CODEX_ACP_BACKEND_ID {
        return Err(agent_session_error(
            "unsupported",
            AgentErrorStage::Configuration,
            "user_action",
            "not_delivered",
            format!(
                "Backend `{}` is not managed by Psychevo; install or repair it with its own package manager.",
                params.id
            ),
            Some(format!("backend:{}", params.id)),
        ));
    }
    let platform = HostPlatform::current();
    let npm = resolve_command_path("npm", &state.inner.inherited_env, &scope.cwd, platform)
        .ok_or_else(|| {
            agent_session_error(
                "npm_missing",
                AgentErrorStage::Configuration,
                "user_action",
                "not_delivered",
                "Node.js/npm is required to install the managed Codex ACP adapter.",
                Some("backend:codex".to_string()),
            )
        })?;
    let before = crate::managed_acp::inspect_managed_codex_acp(&state.inner.home, platform);
    let paths = crate::managed_acp::install_managed_codex_acp(
        &state.inner.home,
        &npm,
        platform,
        &state.inner.inherited_env,
    )
    .await?;
    materialize_local_acp_backends(state, scope)?;
    Ok(serde_json::to_value(wire::BackendManageResult {
        id: params.id,
        operation: operation.to_string(),
        changed: !matches!(before, crate::managed_acp::ManagedCodexAcpStatus::Ready(_))
            || operation != "install",
        status: "ready".to_string(),
        path: paths.root.display().to_string(),
        message: format!(
            "Managed Codex ACP {} is ready.",
            crate::managed_acp::CODEX_ACP_VERSION
        ),
    })?)
}

pub(super) fn backend_doctor_value_for_platform(
    backend: &AgentBackendConfig,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    platform: HostPlatform,
) -> psychevo_runtime::Result<wire::BackendDoctorResult> {
    let mut checks = Vec::new();
    checks.push(wire::BackendDoctorCheck {
        name: "enabled".to_string(),
        ok: backend.enabled,
        message: if backend.enabled {
            "backend enabled"
        } else {
            "backend disabled"
        }
        .to_string(),
        path: None,
    });
    checks.push(wire::BackendDoctorCheck {
        name: "description".to_string(),
        ok: true,
        message: if backend
            .description
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            "description configured"
        } else {
            "description optional; using backend label"
        }
        .to_string(),
        path: None,
    });
    let command_check = match backend.command.as_deref() {
        Some(command) => match resolve_command_path(command, env, cwd, platform) {
            Some(path) => wire::BackendDoctorCheck {
                name: "command".to_string(),
                ok: true,
                message: "command resolved".to_string(),
                path: Some(path.display().to_string()),
            },
            None => wire::BackendDoctorCheck {
                name: "command".to_string(),
                ok: false,
                message: "command was not found on PATH or as a configured path".to_string(),
                path: None,
            },
        },
        None => wire::BackendDoctorCheck {
            name: "command".to_string(),
            ok: false,
            message: "command missing".to_string(),
            path: None,
        },
    };
    checks.push(command_check);
    let ok = checks.iter().all(|check| check.ok);
    Ok(wire::BackendDoctorResult {
        id: backend.id.clone(),
        kind: backend.kind.as_str().to_string(),
        ok,
        checks,
    })
}

pub(super) fn managed_backend_doctor_value(
    state: &WebState,
    scope: &ResolvedScope,
    backend: &AgentBackendConfig,
) -> psychevo_runtime::Result<wire::BackendDoctorResult> {
    let platform = HostPlatform::current();
    let mut result = backend_doctor_value_for_platform(
        backend,
        &state.inner.inherited_env,
        &scope.cwd,
        platform,
    )?;
    if backend.id != crate::managed_acp::CODEX_ACP_BACKEND_ID {
        return Ok(result);
    }
    for program in ["node", "npm"] {
        let resolved =
            resolve_command_path(program, &state.inner.inherited_env, &scope.cwd, platform);
        result.checks.push(wire::BackendDoctorCheck {
            name: program.to_string(),
            ok: resolved.is_some(),
            message: if resolved.is_some() {
                format!("{program} resolved")
            } else {
                format!("{program} is required for managed Codex ACP installation")
            },
            path: resolved.map(|path| path.display().to_string()),
        });
    }
    let managed =
        match crate::managed_acp::inspect_managed_codex_acp_full(&state.inner.home, platform) {
            crate::managed_acp::ManagedCodexAcpStatus::Ready(paths) => wire::BackendDoctorCheck {
                name: "managedAdapter".to_string(),
                ok: true,
                message: format!(
                    "managed Codex ACP {} is installed and verified",
                    crate::managed_acp::CODEX_ACP_VERSION
                ),
                path: Some(paths.executable.display().to_string()),
            },
            crate::managed_acp::ManagedCodexAcpStatus::Missing { paths } => {
                wire::BackendDoctorCheck {
                    name: "managedAdapter".to_string(),
                    ok: false,
                    message: "managed Codex ACP is not installed; run backend/install".to_string(),
                    path: Some(paths.root.display().to_string()),
                }
            }
            crate::managed_acp::ManagedCodexAcpStatus::Invalid { paths, reason } => {
                wire::BackendDoctorCheck {
                    name: "managedAdapter".to_string(),
                    ok: false,
                    message: format!("{reason}; run backend/repair"),
                    path: Some(paths.root.display().to_string()),
                }
            }
        };
    result.checks.push(managed);
    result.ok = result.checks.iter().all(|check| check.ok);
    Ok(result)
}

pub(super) async fn managed_backend_doctor_value_with_auth(
    state: &WebState,
    scope: &ResolvedScope,
    backend: &AgentBackendConfig,
) -> psychevo_runtime::Result<wire::BackendDoctorResult> {
    let mut result = managed_backend_doctor_value(state, scope, backend)?;
    if !result.ok {
        result.checks.push(wire::BackendDoctorCheck {
            name: "protocol".to_string(),
            ok: true,
            message: "protocol compatibility unchecked because backend launch prerequisites failed"
                .to_string(),
            path: None,
        });
        result.checks.push(wire::BackendDoctorCheck {
            name: "authentication".to_string(),
            ok: true,
            message: "authentication unchecked because backend launch prerequisites failed"
                .to_string(),
            path: None,
        });
        return Ok(result);
    }

    let mut options = state.run_options(scope.cwd.clone(), None);
    options.runtime_ref = Some(backend.id.clone());
    let (protocol_check, auth_check) = match crate::resolve_peer_turn(&options) {
        Ok(Some(peer)) => match state
            .inner
            .gateway
            .probe_acp_backend_protocol_compatibility(peer.clone(), scope.cwd.clone())
            .await
        {
            Ok(crate::acp_peer::AcpProtocolDoctorStatus::Compatible { version }) => {
                let protocol_check = wire::BackendDoctorCheck {
                    name: "protocol".to_string(),
                    ok: true,
                    message: format!("stable ACP protocol v{version} negotiated"),
                    path: None,
                };
                let auth_check = match state
                    .inner
                    .gateway
                    .probe_acp_backend_authentication(peer, scope.cwd.clone())
                    .await
                {
                    Ok(crate::acp_peer::AcpAuthDoctorStatus::Authenticated(method)) => {
                        let method = match method {
                            crate::acp_peer::AcpAuthenticatedKind::ApiKey => "api-key",
                            crate::acp_peer::AcpAuthenticatedKind::ChatGpt => "chat-gpt",
                            crate::acp_peer::AcpAuthenticatedKind::Gateway => "gateway",
                        };
                        wire::BackendDoctorCheck {
                            name: "authentication".to_string(),
                            ok: true,
                            message: format!(
                                "authenticated according to Codex ACP authentication/status ({method})"
                            ),
                            path: None,
                        }
                    }
                    Ok(crate::acp_peer::AcpAuthDoctorStatus::Required) => {
                        wire::BackendDoctorCheck {
                            name: "authentication".to_string(),
                            ok: false,
                            message: "authentication required".to_string(),
                            path: None,
                        }
                    }
                    Ok(crate::acp_peer::AcpAuthDoctorStatus::Unchecked) => {
                        wire::BackendDoctorCheck {
                            name: "authentication".to_string(),
                            ok: true,
                            message: "authentication unchecked; this ACP Agent has no source-proven side-effect-free credential-status request"
                                .to_string(),
                            path: None,
                        }
                    }
                    Err(error) => wire::BackendDoctorCheck {
                        name: "authentication".to_string(),
                        ok: false,
                        message: format!(
                            "authentication probe failed: {}",
                            bounded_backend_doctor_message(&error.to_string())
                        ),
                        path: None,
                    },
                };
                (protocol_check, auth_check)
            }
            Ok(crate::acp_peer::AcpProtocolDoctorStatus::Incompatible {
                expected_version,
                actual_version,
            }) => (
                wire::BackendDoctorCheck {
                    name: "protocol".to_string(),
                    ok: false,
                    message: format!(
                        "protocol incompatible: expected stable ACP v{expected_version}, Agent returned v{actual_version}"
                    ),
                    path: None,
                },
                unchecked_authentication_for_protocol(),
            ),
            Err(error) => (
                wire::BackendDoctorCheck {
                    name: "protocol".to_string(),
                    ok: false,
                    message: format!(
                        "protocol probe failed: {}",
                        bounded_backend_doctor_message(&error.to_string())
                    ),
                    path: None,
                },
                unchecked_authentication_for_protocol(),
            ),
        },
        Ok(None) => (
            wire::BackendDoctorCheck {
                name: "protocol".to_string(),
                ok: false,
                message: "protocol probe could not resolve an ACP Agent for this backend"
                    .to_string(),
                path: None,
            },
            unchecked_authentication_for_protocol(),
        ),
        Err(error) => (
            wire::BackendDoctorCheck {
                name: "protocol".to_string(),
                ok: false,
                message: format!(
                    "protocol probe could not resolve the backend: {}",
                    bounded_backend_doctor_message(&error.to_string())
                ),
                path: None,
            },
            unchecked_authentication_for_protocol(),
        ),
    };
    result.checks.push(protocol_check);
    result.checks.push(auth_check);
    result.ok = result.checks.iter().all(|check| check.ok);
    Ok(result)
}

fn unchecked_authentication_for_protocol() -> wire::BackendDoctorCheck {
    wire::BackendDoctorCheck {
        name: "authentication".to_string(),
        ok: true,
        message: "authentication unchecked because ACP protocol is incompatible or unavailable"
            .to_string(),
        path: None,
    }
}

fn bounded_backend_doctor_message(message: &str) -> String {
    message
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(1_024)
        .collect()
}

fn resolve_command_path(
    command: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    platform: HostPlatform,
) -> Option<PathBuf> {
    resolve_executable_path(command, cwd, &ExecutableResolveOptions { platform, env })
}
