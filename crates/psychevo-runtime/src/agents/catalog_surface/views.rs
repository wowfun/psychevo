pub(crate) const MAX_AGENT_NAME_LEN: usize = 64;
pub(crate) const SUBAGENT_TASK_LABEL_MAX_CHARS: usize = 80;
pub(crate) const SUBAGENT_DEFAULT_MAX_TURNS: usize = 32;
pub const MAX_AGENT_SPAWN_DEPTH_CAP: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSource {
    Explicit,
    Project,
    ClaudeProject,
    Global,
    ClaudeGlobal,
    Generated,
    BuiltIn,
}

impl AgentSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Project => "project",
            Self::ClaudeProject => "claude_project",
            Self::Global => "global",
            Self::ClaudeGlobal => "claude_global",
            Self::Generated => "generated",
            Self::BuiltIn => "built_in",
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Project | Self::ClaudeProject => "Project",
            Self::Explicit | Self::Global | Self::ClaudeGlobal | Self::Generated => "User",
            Self::BuiltIn => "System",
        }
    }
}

pub fn agent_source_display_label(value: Option<&str>) -> Option<&'static str> {
    match value.map(str::trim).filter(|value| !value.is_empty())? {
        "project" | "claude_project" | "Project" => Some("Project"),
        "explicit" | "global" | "claude_global" | "generated" | "User" => Some("User"),
        "built_in" | "builtin" | "system" | "core" | "System" => Some("System"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEntrypoint {
    Peer,
    Subagent,
}

impl AgentEntrypoint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Peer => "peer",
            Self::Subagent => "subagent",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "peer" => Some(Self::Peer),
            "subagent" => Some(Self::Subagent),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentBackendKind {
    Acp,
}

impl AgentBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Acp => "acp",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "acp" => Some(Self::Acp),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentBackendRef {
    #[serde(rename = "ref")]
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentBackendConfig {
    pub id: String,
    pub kind: AgentBackendKind,
    pub enabled: bool,
    pub label: String,
    pub description: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
    pub entrypoints: BTreeSet<AgentEntrypoint>,
    pub client_capabilities: BTreeSet<String>,
    pub mcp_servers: BTreeSet<String>,
}

pub(crate) fn default_peer_agent_entrypoints() -> BTreeSet<AgentEntrypoint> {
    [AgentEntrypoint::Peer, AgentEntrypoint::Subagent]
        .into_iter()
        .collect()
}

pub(crate) fn default_subagent_entrypoints() -> BTreeSet<AgentEntrypoint> {
    [AgentEntrypoint::Subagent].into_iter().collect()
}

pub(crate) fn default_peer_client_capabilities() -> BTreeSet<String> {
    ["fs.read", "fs.write", "terminal"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDiagnostic {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl AgentDiagnostic {
    pub(crate) fn warning(message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self {
            kind: "warning".to_string(),
            message: message.into(),
            path,
        }
    }

    pub(crate) fn collision(name: &str, winner: &Path, loser: &Path) -> Self {
        Self {
            kind: "collision".to_string(),
            message: format!(
                "agent name \"{name}\" collision; keeping {} and omitting {}",
                winner.display(),
                loser.display()
            ),
            path: Some(loser.to_path_buf()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentToolPolicy {
    pub allowed: Option<BTreeSet<String>>,
    pub denied: BTreeSet<String>,
    pub allowed_agents: Option<BTreeSet<String>>,
    pub denied_agents: BTreeSet<String>,
    pub permissions: Option<Value>,
    pub permission_mode: Option<AgentPermissionMode>,
    pub mcp_servers: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPermissionMode {
    Default,
    AcceptEdits,
    Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentContribution {
    Instructions,
    Tools,
    Mcp,
    Skills,
}

impl AgentContribution {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "instructions" => Some(Self::Instructions),
            "tools" => Some(Self::Tools),
            "mcp" => Some(Self::Mcp),
            "skills" => Some(Self::Skills),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Instructions => "instructions",
            Self::Tools => "tools",
            Self::Mcp => "mcp",
            Self::Skills => "skills",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub enabled: bool,
    pub file_path: Option<PathBuf>,
    pub source: AgentSource,
    pub backend: Option<AgentBackendRef>,
    pub entrypoints: BTreeSet<AgentEntrypoint>,
    pub model: Option<String>,
    pub tool_policy: AgentToolPolicy,
    pub skills: Vec<String>,
    pub optional_contributions: BTreeSet<AgentContribution>,
    pub hooks: Option<Value>,
    pub background: Option<bool>,
    pub initial_prompt: Option<String>,
    pub max_turns: Option<usize>,
    pub max_spawn_depth: u8,
    pub project_instructions: Option<bool>,
    pub effort: Option<String>,
    pub diagnostics: Vec<AgentDiagnostic>,
}

impl AgentDefinition {
    pub fn supports_entrypoint(&self, entrypoint: AgentEntrypoint) -> bool {
        self.entrypoints.contains(&entrypoint)
    }

    pub fn contribution_is_optional(&self, contribution: AgentContribution) -> bool {
        self.optional_contributions.contains(&contribution)
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentCatalog {
    pub agents: Vec<AgentDefinition>,
    pub shadowed_agents: Vec<AgentDefinition>,
    pub disabled_agents: Vec<AgentDefinition>,
    pub diagnostics: Vec<AgentDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentInvocationRole {
    Main,
    Subagent,
    Fork,
    System,
}

impl AgentInvocationRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Subagent => "subagent",
            Self::Fork => "fork",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentDiscoveryOptions {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub explicit_inputs: Vec<String>,
    pub no_agents: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    PendingInit,
    Running,
    Completed,
    Errored,
    Interrupted,
    Shutdown,
    NotFound,
}
