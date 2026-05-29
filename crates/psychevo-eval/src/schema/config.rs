#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PevalGlobalConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub default_workspace: Option<PathBuf>,
    #[serde(default)]
    pub analysis: Option<PevalAnalysisConfig>,
    #[serde(default)]
    pub reports: BTreeMap<String, PevalReportProfile>,
    #[serde(default)]
    pub agents: Vec<AgentManifest>,
    #[serde(default)]
    pub benchmarks: Vec<RegistryBenchmark>,
}

impl Default for PevalGlobalConfig {
    fn default() -> Self {
        Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            default_workspace: None,
            analysis: None,
            reports: BTreeMap::new(),
            agents: Vec::new(),
            benchmarks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PevalWorkspaceConfig {
    pub schema_version: u32,
    pub kind: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub analysis: Option<PevalAnalysisConfig>,
    #[serde(default)]
    pub reports: BTreeMap<String, PevalReportProfile>,
    #[serde(default)]
    pub agents: Vec<AgentManifest>,
    #[serde(default)]
    pub benchmarks: Vec<RegistryBenchmark>,
}

impl Default for PevalWorkspaceConfig {
    fn default() -> Self {
        Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            kind: "workspace".to_string(),
            name: None,
            analysis: None,
            reports: BTreeMap::new(),
            agents: Vec::new(),
            benchmarks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PevalReportProfile {
    #[serde(default)]
    pub analysis: Option<PevalAnalysisConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PevalAnalysisConfig {
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub concurrency: Option<usize>,
    #[serde(default)]
    pub rubric_path: Option<PathBuf>,
    #[serde(default)]
    pub rubric: Option<PevalAnalysisRubric>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PevalAnalysisRubric {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub checks: Vec<PevalAnalysisRubricCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PevalAnalysisRubricCheck {
    pub name: String,
    pub guidance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryBenchmark {
    pub id: String,
    pub path: PathBuf,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct EvalConfigManifest {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) output_root: Option<PathBuf>,
    pub(crate) artifacts: ArtifactSelection,
    pub(crate) analysis: Option<PevalAnalysisConfig>,
    pub(crate) reports: BTreeMap<String, PevalReportProfile>,
    pub(crate) benchmark: BenchmarkReference,
    pub(crate) selection: EvalSelection,
    pub(crate) agents: Vec<AgentManifest>,
    pub(crate) benchmarks: Vec<RegistryBenchmark>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawEvalConfigManifest {
    pub(crate) schema_version: u32,
    #[serde(default = "default_eval_id")]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) output_root: Option<PathBuf>,
    #[serde(default)]
    pub(crate) artifacts: ArtifactSelection,
    #[serde(default)]
    pub(crate) analysis: Option<PevalAnalysisConfig>,
    #[serde(default)]
    pub(crate) reports: BTreeMap<String, PevalReportProfile>,
    pub(crate) benchmark: BenchmarkReference,
    pub(crate) select: EvalSelection,
    #[serde(default)]
    pub(crate) agents: Vec<AgentManifest>,
    #[serde(default)]
    pub(crate) benchmarks: Vec<RegistryBenchmark>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkReference {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawBenchmarkManifestSerde {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) sources: BenchmarkSources,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ManifestVersion {
    pub(crate) schema_version: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BenchmarkSources {
    #[serde(default)]
    pub(crate) peval_agent: Vec<PevalAgentSourceManifest>,
    #[serde(default)]
    pub(crate) harbor: Vec<HarborSourceManifest>,
    #[serde(default)]
    pub(crate) swe_bench: Vec<SweBenchSourceManifest>,
    #[serde(default)]
    pub(crate) tau2: Vec<Tau2SourceManifest>,
}

impl BenchmarkSources {
    pub(crate) fn is_empty(&self) -> bool {
        self.peval_agent.is_empty()
            && self.harbor.is_empty()
            && self.swe_bench.is_empty()
            && self.tau2.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PevalAgentSourceManifest {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    #[serde(default)]
    pub(crate) execution: ExecutionBackend,
    #[serde(default)]
    pub(crate) verifier_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) sets: Vec<SourceSetManifest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HarborSourceManifest {
    pub(crate) id: String,
    pub(crate) root: PathBuf,
    pub(crate) path: PathBuf,
    #[serde(default)]
    pub(crate) execution: ExecutionBackend,
    #[serde(default)]
    pub(crate) sets: Vec<SourceSetManifest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SweBenchSourceManifest {
    pub(crate) id: String,
    pub(crate) root: PathBuf,
    pub(crate) dataset: String,
    pub(crate) split: String,
    #[serde(default)]
    pub(crate) execution: ExecutionBackend,
    #[serde(default)]
    pub(crate) sets: Vec<SourceSetManifest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tau2SourceManifest {
    pub(crate) id: String,
    pub(crate) root: PathBuf,
    pub(crate) domain: String,
    #[serde(default)]
    pub(crate) execution: ExecutionBackend,
    #[serde(default)]
    pub(crate) split: Option<String>,
    #[serde(default)]
    pub(crate) task_set: Option<String>,
    #[serde(default)]
    pub(crate) sets: Vec<SourceSetManifest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceSetManifest {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) include: Vec<String>,
    #[serde(default)]
    pub(crate) exclude: Vec<String>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

pub(crate) fn default_eval_id() -> String {
    "evaluation".to_string()
}
