use super::{
    AgentMissionRunRecord, AgentTeamRunRecord, BTreeMap, BTreeSet, Deserialize, Error, Path,
    PathBuf, Result, Serialize, SqliteStore, Value, fs,
};
use super::{
    catalog_surface::{AgentCatalog, AgentDiagnostic, AgentDiscoveryOptions, discover_agents},
    definition_policy::{split_frontmatter, valid_agent_name},
};

pub const DEFAULT_TEAM_PARALLEL_AGENTS: u64 = 4;
pub const MAX_TEAM_PARALLEL_AGENTS_CAP: u64 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTeamSource {
    Project,
    Profile,
}

impl AgentTeamSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Profile => "profile",
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::Profile => "User",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeamMember {
    pub id: String,
    pub agent: String,
    /// Runtime Profile selected independently from the Agent Definition.
    ///
    /// `None` keeps the existing Psychevo-managed child execution path. A
    /// concrete value is an execution identity, not an agent/backend name.
    #[serde(default)]
    pub runtime_ref: Option<String>,
    /// Advanced per-member overrides interpreted by the selected Runtime
    /// Profile. Values stay strings because the runtime control surface is
    /// responsible for validating them against its captured snapshot.
    #[serde(default)]
    pub runtime_options: BTreeMap<String, String>,
    /// Effective Runtime Profile revision captured when the Team definition is
    /// validated or activated. Runtime-backed execution must reject a stale
    /// value instead of silently adopting a changed Profile.
    #[serde(default)]
    pub runtime_profile_revision: Option<u64>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub max_turns: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentTeamDefinition {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub file_path: Option<PathBuf>,
    pub source: AgentTeamSource,
    pub leader: String,
    pub members: Vec<AgentTeamMember>,
    pub max_parallel_agents: u64,
    pub instructions: String,
    pub diagnostics: Vec<AgentDiagnostic>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentTeamCatalog {
    pub teams: Vec<AgentTeamDefinition>,
    pub shadowed_teams: Vec<AgentTeamDefinition>,
    pub disabled_teams: Vec<AgentTeamDefinition>,
    pub diagnostics: Vec<AgentDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAgentTeamContext {
    pub team_run_id: String,
    #[serde(default)]
    pub mission_run_id: Option<String>,
    pub team_name: String,
    #[serde(default)]
    pub mission_goal: Option<String>,
    pub leader_agent_name: String,
    pub max_parallel_agents: u64,
    #[serde(default)]
    pub members: Vec<AgentTeamMember>,
}

impl ActiveAgentTeamContext {
    pub fn member(&self, id: &str) -> Option<&AgentTeamMember> {
        self.members.iter().find(|member| member.id == id)
    }
}

pub fn active_agent_team_context_for_session(
    store: &SqliteStore,
    parent_session_id: &str,
) -> Result<Option<ActiveAgentTeamContext>> {
    let Some(team) = store.find_active_agent_team_run(parent_session_id)? else {
        return Ok(None);
    };
    let mission = store.find_active_agent_mission_run(parent_session_id)?;
    active_agent_team_context_from_runs(team, mission)
}

pub fn active_agent_team_context_from_runs(
    team: AgentTeamRunRecord,
    mission: Option<AgentMissionRunRecord>,
) -> Result<Option<ActiveAgentTeamContext>> {
    let members = serde_json::from_value::<Vec<AgentTeamMember>>(team.members.clone())
        .map_err(|err| Error::Config(format!("team run members failed: {err}")))?;
    Ok(Some(ActiveAgentTeamContext {
        team_run_id: team.id,
        mission_run_id: mission
            .as_ref()
            .map(|mission| mission.id.clone())
            .or(team.mission_run_id),
        team_name: team.team_name,
        mission_goal: mission.map(|mission| mission.goal),
        leader_agent_name: team.leader_agent_name,
        max_parallel_agents: team.max_parallel_agents,
        members,
    }))
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawAgentTeamFrontmatter {
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) enabled: Option<Value>,
    pub(crate) leader: Option<String>,
    pub(crate) members: Option<Value>,
    #[serde(rename = "maxParallelAgents", alias = "max_parallel_agents")]
    pub(crate) max_parallel_agents: Option<Value>,
    pub(crate) isolation: Option<Value>,
}

pub fn discover_agent_teams(options: &AgentDiscoveryOptions) -> Result<AgentTeamCatalog> {
    let agents = discover_agents(options)?;
    discover_agent_teams_with_catalog(options, &agents)
}

pub fn discover_agent_teams_with_catalog(
    options: &AgentDiscoveryOptions,
    agents: &AgentCatalog,
) -> Result<AgentTeamCatalog> {
    if options.no_agents {
        return Ok(AgentTeamCatalog::default());
    }
    let mut catalog = AgentTeamCatalog::default();
    let mut winners: BTreeMap<String, PathBuf> = BTreeMap::new();

    load_team_dir(
        &mut catalog,
        &mut winners,
        &options.cwd.join(".psychevo").join("teams"),
        AgentTeamSource::Project,
        agents,
    )?;
    load_team_dir(
        &mut catalog,
        &mut winners,
        &options.home.join("teams"),
        AgentTeamSource::Profile,
        agents,
    )?;

    Ok(catalog)
}

pub fn parse_agent_team_definition_text(
    content: &str,
    default_name: &str,
    file_path: Option<PathBuf>,
    source: AgentTeamSource,
    agents: &AgentCatalog,
) -> Result<AgentTeamDefinition> {
    let (frontmatter, instructions) = split_frontmatter(content)?;
    let raw = match frontmatter {
        Some(frontmatter) => serde_yaml::from_str::<RawAgentTeamFrontmatter>(frontmatter)
            .map_err(|err| Error::Config(format!("team frontmatter failed: {err}")))?,
        None => RawAgentTeamFrontmatter::default(),
    };
    team_from_raw(raw, default_name, instructions, file_path, source, agents)
}

pub(crate) fn parse_team_file(
    path: &Path,
    source: AgentTeamSource,
    agents: &AgentCatalog,
) -> Result<AgentTeamDefinition> {
    let content = fs::read_to_string(path)?;
    parse_agent_team_definition_text(
        &content,
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("team"),
        Some(path.to_path_buf()),
        source,
        agents,
    )
}

pub(crate) fn team_from_raw(
    raw: RawAgentTeamFrontmatter,
    default_name: &str,
    instructions: String,
    file_path: Option<PathBuf>,
    source: AgentTeamSource,
    agents: &AgentCatalog,
) -> Result<AgentTeamDefinition> {
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
            format!("team name `{name}` is invalid"),
            path.clone(),
        ));
    }
    let description = raw
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config(format!("team `{name}` must define a description")))?
        .to_string();
    let leader = raw
        .leader
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config(format!("team `{name}` must define a leader")))?
        .to_string();
    if !valid_agent_name(&leader) {
        diagnostics.push(AgentDiagnostic::warning(
            format!("team `{name}` leader `{leader}` is invalid"),
            path.clone(),
        ));
    }
    if !agent_catalog_contains(agents, &leader) {
        diagnostics.push(AgentDiagnostic::warning(
            format!("team `{name}` references unknown leader agent `{leader}`"),
            path.clone(),
        ));
    }
    let members = parse_team_members(raw.members.as_ref(), &name, path.clone())?;
    if members.is_empty() {
        return Err(Error::Config(format!(
            "team `{name}` must define at least one member"
        )));
    }
    for member in &members {
        if !valid_agent_name(&member.id) {
            diagnostics.push(AgentDiagnostic::warning(
                format!("team `{name}` member id `{}` is invalid", member.id),
                path.clone(),
            ));
        }
        if !valid_agent_name(&member.agent) {
            diagnostics.push(AgentDiagnostic::warning(
                format!(
                    "team `{name}` member `{}` references invalid agent `{}`",
                    member.id, member.agent
                ),
                path.clone(),
            ));
        }
        if !agent_catalog_contains(agents, &member.agent) {
            diagnostics.push(AgentDiagnostic::warning(
                format!(
                    "team `{name}` member `{}` references unknown agent `{}`",
                    member.id, member.agent
                ),
                path.clone(),
            ));
        }
    }
    if raw
        .isolation
        .as_ref()
        .is_some_and(|value| value.as_str() == Some("worktree"))
    {
        diagnostics.push(AgentDiagnostic::warning(
            "team isolation: worktree is parsed but not executed in this version",
            path.clone(),
        ));
    }
    let (enabled, enabled_diagnostic) = parse_team_enabled(raw.enabled.as_ref());
    if let Some(message) = enabled_diagnostic {
        diagnostics.push(AgentDiagnostic::warning(message, path.clone()));
    }
    let (max_parallel_agents, cap_diagnostic) =
        parse_max_parallel_agents(raw.max_parallel_agents.as_ref());
    if let Some(message) = cap_diagnostic {
        diagnostics.push(AgentDiagnostic::warning(message, path.clone()));
    }

    Ok(AgentTeamDefinition {
        name,
        description,
        enabled,
        file_path,
        source,
        leader,
        members,
        max_parallel_agents,
        instructions: instructions.trim().to_string(),
        diagnostics,
    })
}

fn parse_team_members(
    value: Option<&Value>,
    team_name: &str,
    path: Option<PathBuf>,
) -> Result<Vec<AgentTeamMember>> {
    let Some(value) = value else {
        return Err(Error::Config(format!(
            "team `{team_name}` must define members"
        )));
    };
    let items = value
        .as_array()
        .ok_or_else(|| Error::Config(format!("team `{team_name}` members must be an array")))?;
    let mut members = Vec::new();
    let mut ids = BTreeSet::new();
    for item in items {
        let member = if let Some(agent) = item.as_str() {
            AgentTeamMember {
                id: agent.trim().to_string(),
                agent: agent.trim().to_string(),
                runtime_ref: None,
                runtime_options: BTreeMap::new(),
                runtime_profile_revision: None,
                role: None,
                description: None,
                max_turns: None,
            }
        } else {
            serde_json::from_value::<AgentTeamMember>(item.clone())
                .map_err(|err| Error::Config(format!("team `{team_name}` member failed: {err}")))?
        };
        if member.id.trim().is_empty() || member.agent.trim().is_empty() {
            return Err(Error::Config(format!(
                "team `{team_name}` member id and agent must be non-empty"
            )));
        }
        let runtime_ref = member
            .runtime_ref
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if let Some(runtime_ref) = runtime_ref.as_deref()
            && !valid_team_runtime_ref(runtime_ref)
        {
            return Err(Error::Config(format!(
                "team `{team_name}` member `{}` runtimeRef `{runtime_ref}` must be a Runtime Profile id or generated acp:<backend-id> id",
                member.id
            )));
        }
        if runtime_ref.is_none() && !member.runtime_options.is_empty() {
            return Err(Error::Config(format!(
                "team `{team_name}` member `{}` runtimeOptions require runtimeRef",
                member.id
            )));
        }
        if runtime_ref.is_none() && member.runtime_profile_revision.is_some() {
            return Err(Error::Config(format!(
                "team `{team_name}` member `{}` runtimeProfileRevision requires runtimeRef",
                member.id
            )));
        }
        let mut runtime_options = BTreeMap::new();
        for (raw_key, value) in member.runtime_options {
            let key = raw_key.trim();
            if key.is_empty() {
                return Err(Error::Config(format!(
                    "team `{team_name}` member `{}` runtimeOptions keys must be non-empty",
                    member.id
                )));
            }
            if runtime_options.insert(key.to_string(), value).is_some() {
                return Err(Error::Config(format!(
                    "team `{team_name}` member `{}` runtimeOptions contain duplicate key `{key}` after normalization",
                    member.id
                )));
            }
        }
        let member = AgentTeamMember {
            id: member.id.trim().to_string(),
            agent: member.agent.trim().to_string(),
            runtime_ref,
            runtime_options,
            runtime_profile_revision: member.runtime_profile_revision,
            role: member
                .role
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            description: member
                .description
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            max_turns: member.max_turns,
        };
        if !ids.insert(member.id.clone()) {
            return Err(Error::Config(format!(
                "team `{team_name}` has duplicate member id `{}`",
                member.id
            )));
        }
        members.push(member);
    }
    let _ = path;
    Ok(members)
}

fn valid_team_runtime_ref(value: &str) -> bool {
    valid_agent_name(value) || value.strip_prefix("acp:").is_some_and(valid_agent_name)
}

fn parse_team_enabled(value: Option<&Value>) -> (bool, Option<String>) {
    match value {
        None => (true, None),
        Some(Value::Bool(value)) => (*value, None),
        Some(_) => (
            true,
            Some("team enabled must be a boolean; defaulting to true".to_string()),
        ),
    }
}

fn parse_max_parallel_agents(value: Option<&Value>) -> (u64, Option<String>) {
    let Some(value) = value else {
        return (DEFAULT_TEAM_PARALLEL_AGENTS, None);
    };
    let raw = value.as_u64().or_else(|| {
        value
            .as_i64()
            .and_then(|value| (value > 0).then_some(value as u64))
    });
    let Some(raw) = raw else {
        return (
            DEFAULT_TEAM_PARALLEL_AGENTS,
            Some("team maxParallelAgents must be a positive integer; defaulting to 4".to_string()),
        );
    };
    let clamped = raw.clamp(1, MAX_TEAM_PARALLEL_AGENTS_CAP);
    let diagnostic = (raw != clamped).then(|| {
        format!("team maxParallelAgents {raw} exceeds cap {MAX_TEAM_PARALLEL_AGENTS_CAP}; clamped")
    });
    (clamped, diagnostic)
}

fn agent_catalog_contains(catalog: &AgentCatalog, name: &str) -> bool {
    catalog.agents.iter().any(|agent| agent.name == name)
}

fn load_team_dir(
    catalog: &mut AgentTeamCatalog,
    winners: &mut BTreeMap<String, PathBuf>,
    dir: &Path,
    source: AgentTeamSource,
    agents: &AgentCatalog,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for path in team_markdown_files(dir)? {
        match parse_team_file(&path, source, agents) {
            Ok(team) => insert_team(catalog, winners, team),
            Err(err) => catalog.diagnostics.push(AgentDiagnostic::warning(
                format!("failed to load team {}: {err}", path.display()),
                Some(path),
            )),
        }
    }
    Ok(())
}

fn team_markdown_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_team_markdown_files(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_team_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_team_markdown_files(&path, files)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("md") {
            files.push(path);
        }
    }
    Ok(())
}

fn insert_team(
    catalog: &mut AgentTeamCatalog,
    winners: &mut BTreeMap<String, PathBuf>,
    team: AgentTeamDefinition,
) {
    if !team.enabled {
        catalog.disabled_teams.push(team);
        return;
    }
    let path = team.file_path.clone().unwrap_or_default();
    if let Some(winner) = winners.get(&team.name) {
        let mut shadowed = team;
        shadowed
            .diagnostics
            .push(AgentDiagnostic::collision(&shadowed.name, winner, &path));
        catalog.shadowed_teams.push(shadowed);
        return;
    }
    winners.insert(team.name.clone(), path);
    catalog.teams.push(team);
}

pub fn resolve_agent_team_definition(
    catalog: &AgentTeamCatalog,
    input: &str,
) -> Result<AgentTeamDefinition> {
    catalog
        .teams
        .iter()
        .find(|team| team.name == input)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown team: {input}")))
}
