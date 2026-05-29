#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct EvalProject {
    pub eval_root: Option<PathBuf>,
    pub eval_manifest_path: Option<PathBuf>,
    pub id: String,
    pub name: String,
    pub benchmark_root: PathBuf,
    pub benchmark_manifest_path: PathBuf,
    pub benchmark_id: String,
    pub benchmark_name: String,
    pub schema_version: u32,
    pub output_root: Option<PathBuf>,
    pub artifacts: ArtifactSelection,
    pub agents: BTreeMap<String, AgentManifest>,
    pub task_sets: BTreeMap<String, TaskSetManifest>,
    pub tasks: BTreeMap<String, TaskManifest>,
    pub selection: EvalSelection,
}

#[derive(Debug, Clone)]
pub struct BenchmarkManifest {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub sources: Vec<BenchmarkSourceSummary>,
    pub task_sets: BTreeMap<String, TaskSetManifest>,
    pub tasks: BTreeMap<String, TaskManifest>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalSelection {
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub sets: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArtifactSelection {
    #[serde(default)]
    pub include: Vec<String>,
}

impl EvalSelection {
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty() && self.sets.is_empty() && self.tasks.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    #[serde(default = "current_manifest_schema_version")]
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub kind: AgentKind,
    #[serde(default)]
    pub fake: FakeAgentOptions,
    #[serde(default)]
    pub command: CommandAgentOptions,
    #[serde(default)]
    pub acp: AcpAgentOptions,
    #[serde(default)]
    pub psychevo: PsychevoAgentOptions,
    #[serde(default)]
    pub opencode: WrapperAgentOptions,
    #[serde(default)]
    pub hermes: WrapperAgentOptions,
    #[serde(skip)]
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    Fake,
    Command,
    Acp,
    PsychevoAcp,
    OpencodeAcp,
    HermesAcp,
    HumanInLoop,
    Psychevo,
    Opencode,
    Hermes,
}

impl AgentKind {
    pub fn is_acp_adapter(self) -> bool {
        matches!(
            self,
            AgentKind::Acp | AgentKind::PsychevoAcp | AgentKind::OpencodeAcp | AgentKind::HermesAcp
        )
    }

    pub fn is_removed_wrapper(self) -> bool {
        matches!(
            self,
            AgentKind::Psychevo | AgentKind::Opencode | AgentKind::Hermes
        )
    }

    pub fn migration_hint(self) -> Option<&'static str> {
        match self {
            AgentKind::Psychevo => Some("use kind = \"psychevo-acp\" with [agents.acp]"),
            AgentKind::Opencode => Some("use kind = \"opencode-acp\" with [agents.acp]"),
            AgentKind::Hermes => Some("use kind = \"hermes-acp\" with [agents.acp]"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FakeAgentOptions {
    #[serde(default = "default_fake_behavior")]
    pub behavior: FakeBehavior,
}

impl Default for FakeAgentOptions {
    fn default() -> Self {
        Self {
            behavior: FakeBehavior::Pass,
        }
    }
}

pub(crate) fn default_fake_behavior() -> FakeBehavior {
    FakeBehavior::Pass
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FakeBehavior {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PsychevoAgentOptions {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandAgentOptions {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_agent_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub model: Option<String>,
}

impl Default for CommandAgentOptions {
    fn default() -> Self {
        Self {
            command: None,
            args: Vec::new(),
            timeout_seconds: default_agent_timeout_seconds(),
            model: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpAgentOptions {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_agent_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub permission: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub install: AcpInstallOptions,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub binary: Option<PathBuf>,
}

impl Default for AcpAgentOptions {
    fn default() -> Self {
        Self {
            command: None,
            args: Vec::new(),
            timeout_seconds: default_agent_timeout_seconds(),
            model: None,
            mode: None,
            permission: None,
            env: BTreeMap::new(),
            install: AcpInstallOptions::default(),
            version: None,
            binary: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpInstallStrategy {
    ProfileDefault,
    Preinstalled,
    InstallCommand,
    CopyBinary,
}

impl Default for AcpInstallStrategy {
    fn default() -> Self {
        Self::ProfileDefault
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpInstallOptions {
    #[serde(default)]
    pub strategy: AcpInstallStrategy,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_acp_install_cache")]
    pub cache: bool,
}

impl Default for AcpInstallOptions {
    fn default() -> Self {
        Self {
            strategy: AcpInstallStrategy::default(),
            command: None,
            args: Vec::new(),
            cache: default_acp_install_cache(),
        }
    }
}

pub(crate) fn default_acp_install_cache() -> bool {
    true
}

pub(crate) fn default_agent_timeout_seconds() -> u64 {
    600
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WrapperAgentOptions {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub collector: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSetManifest {
    #[serde(default = "current_manifest_schema_version")]
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tasks: Vec<String>,
    #[serde(skip)]
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskSourceKind {
    #[default]
    PevalAgent,
    Harbor,
    SweBench,
    Tau2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionBackend {
    #[default]
    Auto,
    Local,
    Container,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSourceSummary {
    pub id: String,
    pub kind: TaskSourceKind,
    #[serde(default)]
    pub execution: ExecutionBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskManifest {
    #[serde(default = "current_manifest_schema_version")]
    pub schema_version: u32,
    #[serde(rename = "task_id")]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_task_kind")]
    pub kind: String,
    pub problem_statement: String,
    pub workspace: WorkspaceManifest,
    pub test_spec: TestSpecManifest,
    #[serde(default)]
    pub source_kind: TaskSourceKind,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub native_id: String,
    #[serde(default)]
    pub execution: ExecutionBackend,
    #[serde(default)]
    pub verifier_timeout_seconds: Option<u64>,
    #[serde(skip)]
    pub manifest_path: PathBuf,
    #[serde(skip)]
    pub dir: PathBuf,
}

pub(crate) fn current_manifest_schema_version() -> u32 {
    MANIFEST_SCHEMA_VERSION
}

pub(crate) fn default_task_kind() -> String {
    "swe-style".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub source: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSpecManifest {
    #[serde(default)]
    pub checks: Vec<LocalCodingCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LocalCodingCheck {
    PythonFunctionCases {
        module: PathBuf,
        function: String,
        cases: Vec<PythonFunctionCase>,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    ExactFile {
        path: PathBuf,
        expected: String,
    },
    CargoTest {
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonFunctionCase {
    #[serde(default)]
    pub args: Vec<Value>,
    #[serde(default)]
    pub kwargs: BTreeMap<String, Value>,
    pub expected: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandManifest {
    pub command: Vec<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FakeTaskCommands {
    #[serde(default)]
    pub pass: Option<CommandManifest>,
    #[serde(default)]
    pub fail: Option<CommandManifest>,
}
