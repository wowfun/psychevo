#[allow(unused_imports)]
pub(crate) use super::*;

#[allow(unused_imports)]
use anyhow::{Context, Result, bail};
#[allow(unused_imports)]
use clap::{Parser, Subcommand, ValueEnum};
#[allow(unused_imports)]
use serde_json::{Value, json};
#[allow(unused_imports)]
use std::collections::{BTreeMap, BTreeSet};
#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::ffi::OsString;
#[allow(unused_imports)]
use std::fs;
#[allow(unused_imports)]
use std::io::{BufRead, BufReader};
#[allow(unused_imports)]
use std::path::{Component, Path, PathBuf};
#[allow(unused_imports)]
use std::process::{Command, Stdio};
#[allow(unused_imports)]
use std::thread;
#[allow(unused_imports)]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[allow(unused_imports)]
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct EvalProject {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub schema_version: u32,
    pub name: String,
    pub output_root: Option<PathBuf>,
    pub allow_live: bool,
    pub agents: BTreeMap<String, AgentManifest>,
    pub suites: BTreeMap<String, SuiteManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub kind: AgentKind,
    #[serde(default)]
    pub fake: FakeAgentOptions,
    #[serde(default)]
    pub psychevo: PsychevoAgentOptions,
    #[serde(skip)]
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    Fake,
    Psychevo,
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
pub struct SuiteManifest {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<PathBuf>,
    #[serde(skip)]
    pub manifest_path: PathBuf,
    #[serde(skip)]
    pub dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskManifest {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_task_kind")]
    pub kind: String,
    pub prompt: PromptManifest,
    pub workspace: WorkspaceManifest,
    pub scorer: CommandManifest,
    #[serde(default)]
    pub fake: FakeTaskCommands,
    #[serde(skip)]
    pub manifest_path: PathBuf,
    #[serde(skip)]
    pub dir: PathBuf,
}

pub(crate) fn default_task_kind() -> String {
    "swe-style".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptManifest {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub source: PathBuf,
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

#[derive(Debug, Clone)]
pub struct RunRequest {
    pub config: Option<PathBuf>,
    pub suite: Option<String>,
    pub agent: Option<String>,
    pub run_id: Option<String>,
    pub store_root: Option<PathBuf>,
    pub output_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct InitStoreRequest {
    pub root: Option<PathBuf>,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub schema_version: u32,
    pub run_id: String,
    pub project: String,
    pub artifact_root: PathBuf,
    pub started_at_ms: u128,
    pub finished_at_ms: u128,
    pub total_cases: usize,
    pub passed_cases: usize,
    pub failed_cases: usize,
    pub status: RunStatus,
    pub cases: Vec<CaseResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "kebab-case")]
pub enum RunStatusFilter {
    Passed,
    Failed,
}

impl From<RunStatusFilter> for RunStatus {
    fn from(value: RunStatusFilter) -> Self {
        match value {
            RunStatusFilter::Passed => RunStatus::Passed,
            RunStatusFilter::Failed => RunStatus::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    pub schema_version: u32,
    pub case_id: String,
    pub suite_id: String,
    pub task_id: String,
    #[serde(default = "default_task_kind")]
    pub task_family: String,
    pub agent_id: String,
    pub status: CaseStatus,
    #[serde(default)]
    pub failure_class: Option<String>,
    pub score: ScoreResult,
    pub duration_ms: u128,
    pub artifacts: CaseArtifacts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseStatus {
    Passed,
    Failed,
    SetupFailed,
    RuntimeFailed,
    ScorerFailed,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreResult {
    pub schema_version: u32,
    pub passed: bool,
    #[serde(default)]
    pub score: Option<f64>,
    pub message: String,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseArtifacts {
    pub result: PathBuf,
    pub trajectory: PathBuf,
    pub scorer_stdout: PathBuf,
    pub scorer_stderr: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryEvent {
    pub schema_version: u32,
    pub sequence: u64,
    pub case_id: String,
    pub kind: String,
    pub message: String,
    pub timestamp_ms: u128,
    #[serde(default)]
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct ReportRequest {
    pub run_root: PathBuf,
    pub format: ReportFormat,
}

#[derive(Debug, Clone)]
pub struct CompareRequest {
    pub run_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ReplayRequest {
    pub run_root: PathBuf,
    pub case_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareReport {
    pub schema_version: u32,
    pub runs: Vec<CompareRun>,
    pub cases: Vec<CompareCase>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareRun {
    pub run_id: String,
    pub artifact_root: PathBuf,
    pub status: RunStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareCase {
    pub key: String,
    pub statuses: BTreeMap<String, CaseStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayReport {
    pub schema_version: u32,
    pub run_id: String,
    pub events: Vec<TrajectoryEvent>,
}

#[derive(Debug, Clone, Default)]
pub struct RunSelectorFilters {
    pub suite: Option<String>,
    pub agent: Option<String>,
    pub status: Option<RunStatusFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunIndexEntry {
    pub schema_version: u32,
    pub project: String,
    pub project_slug: String,
    #[serde(default)]
    pub namespace: PathBuf,
    pub run_id: String,
    pub artifact_root: PathBuf,
    pub report_html: PathBuf,
    pub report_markdown: PathBuf,
    pub started_at_ms: u128,
    pub finished_at_ms: u128,
    pub total_cases: usize,
    pub passed_cases: usize,
    pub failed_cases: usize,
    pub status: RunStatus,
    pub suites: Vec<String>,
    pub agents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalStoreIndex {
    pub schema_version: u32,
    pub generated_at_ms: u128,
    pub runs: Vec<RunIndexEntry>,
    pub datasets: Vec<DatasetEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestProjectIndex {
    pub schema_version: u32,
    pub generated_at_ms: u128,
    pub project: String,
    pub project_slug: String,
    pub latest: Option<RunIndexEntry>,
    pub runs: Vec<RunIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub payload: PathBuf,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub split: Option<String>,
    #[serde(default)]
    pub sample_limit: Option<usize>,
    #[serde(default)]
    pub cache_key: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetEntry {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub payload: PathBuf,
    pub payload_exists: bool,
    pub manifest_path: PathBuf,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub split: Option<String>,
    #[serde(default)]
    pub sample_limit: Option<usize>,
    #[serde(default)]
    pub cache_key: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DatasetImportRequest {
    pub store_root: Option<PathBuf>,
    pub path: PathBuf,
    pub id: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub loader: Option<String>,
    pub split: Option<String>,
    pub sample_limit: Option<usize>,
    pub cache_key: Option<String>,
    pub license: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EvalStore {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PevalConfig {
    pub schema_version: u32,
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectManifest {
    pub(crate) schema_version: u32,
    pub(crate) name: String,
    pub(crate) output_root: Option<PathBuf>,
    pub(crate) allow_live: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawProjectManifest {
    pub(crate) schema_version: u32,
    #[serde(default = "default_project_name")]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) output_root: Option<PathBuf>,
    #[serde(default)]
    pub(crate) allow_live: bool,
}

pub(crate) fn default_project_name() -> String {
    "evaluation".to_string()
}

#[derive(Debug, Clone)]
pub struct CasePlan {
    pub case_id: String,
    pub suite: SuiteManifest,
    pub task: TaskManifest,
    pub agent: AgentManifest,
}

#[derive(Debug)]
pub(crate) struct ProcessOutcome {
    pub(crate) success: bool,
    pub(crate) code: Option<i32>,
    pub(crate) timed_out: bool,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl EvalProject {
    pub fn load(start: impl AsRef<Path>) -> Result<Self> {
        let manifest_path = discover_manifest(start.as_ref())?;
        let root = manifest_path
            .parent()
            .context("eval.toml has no parent directory")?
            .to_path_buf();
        let manifest = read_project_manifest(&manifest_path)?;
        let agents = load_agent_manifests(&root)?;
        let suites = load_suite_manifests(&root)?;
        Ok(Self {
            root,
            manifest_path,
            schema_version: manifest.schema_version,
            name: manifest.name,
            output_root: manifest.output_root,
            allow_live: manifest.allow_live,
            agents,
            suites,
        })
    }

    pub fn namespace(&self) -> Result<PathBuf> {
        self.output_root
            .as_ref()
            .map(|path| validate_store_namespace(path))
            .transpose()?
            .map(Ok)
            .unwrap_or_else(|| Ok(PathBuf::from("runs").join(self.slug())))
    }

    pub fn slug(&self) -> String {
        slugify(&self.name)
    }
}

impl EvalStore {
    pub fn resolve(store_root: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            root: resolve_store_root(store_root)?,
        })
    }

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn run_output_base(&self, project: &EvalProject) -> Result<PathBuf> {
        Ok(self.root.join(project.namespace()?))
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.root.join("runs"))
            .with_context(|| format!("failed to create {}", self.root.join("runs").display()))?;
        fs::create_dir_all(self.root.join("datasets")).with_context(|| {
            format!("failed to create {}", self.root.join("datasets").display())
        })?;
        self.refresh_indexes()?;
        self.write_dashboard()
    }

    pub fn resolve_run_selector(
        &self,
        namespace: Option<&Path>,
        selector: &Path,
        filters: &RunSelectorFilters,
    ) -> Result<PathBuf> {
        let explicit_selector = resolve_cli_path(selector)?;
        if explicit_selector.join("summary.json").is_file() {
            return Ok(explicit_selector);
        }

        let selector_text = selector.to_string_lossy();
        if selector_text == "latest" {
            return self
                .latest_run(namespace, filters)?
                .map(|entry| entry.artifact_root)
                .with_context(|| {
                    let scope = namespace
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "global".to_string());
                    format!(
                        "no latest run found for {scope} under {}",
                        self.root.display()
                    )
                });
        }

        if let Some(namespace) = namespace
            && selector.components().count() == 1
        {
            let run_root = self.root.join(namespace).join(selector);
            if run_root.join("summary.json").is_file() {
                return Ok(run_root);
            }
        }

        let store_root = self.root.join(selector);
        if store_root.join("summary.json").is_file() {
            return Ok(store_root);
        }

        let legacy_runs_root = self.root.join("runs").join(selector);
        if legacy_runs_root.join("summary.json").is_file() {
            return Ok(legacy_runs_root);
        }

        bail!(
            "could not resolve run selector `{}` under {}",
            selector.display(),
            self.root.display()
        )
    }

    pub fn list_runs(&self) -> Result<Vec<RunIndexEntry>> {
        match self.read_index() {
            Ok(index) => Ok(index.runs),
            Err(_) => self.scan_runs(),
        }
    }

    pub fn list_datasets(&self) -> Result<Vec<DatasetEntry>> {
        let mut entries = Vec::new();
        let datasets_dir = self.root.join("datasets");
        if !datasets_dir.is_dir() {
            return Ok(entries);
        }
        for entry in fs::read_dir(&datasets_dir)
            .with_context(|| format!("failed to read {}", datasets_dir.display()))?
        {
            let path = entry?.path().join("dataset.toml");
            if path.is_file() {
                entries.push(read_dataset_entry(&path)?);
            }
        }
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(entries)
    }

    pub fn register_run(&self, _summary: &RunSummary) -> Result<()> {
        self.refresh_indexes()?;
        self.write_dashboard().with_context(|| {
            format!(
                "failed to write {}",
                self.root.join("dashboard.html").display()
            )
        })
    }

    pub fn refresh_after_dataset_change(&self) -> Result<()> {
        self.refresh_indexes()?;
        self.write_dashboard()
    }

    pub(crate) fn latest_run(
        &self,
        namespace: Option<&Path>,
        filters: &RunSelectorFilters,
    ) -> Result<Option<RunIndexEntry>> {
        let status = filters.status.map(RunStatus::from);
        let mut runs = self.list_runs()?;
        runs.retain(|entry| {
            namespace.is_none_or(|expected| entry.namespace == expected)
                && filters
                    .suite
                    .as_ref()
                    .is_none_or(|suite| entry.suites.iter().any(|value| value == suite))
                && filters
                    .agent
                    .as_ref()
                    .is_none_or(|agent| entry.agents.iter().any(|value| value == agent))
                && status.is_none_or(|expected| entry.status == expected)
        });
        runs.sort_by(|left, right| {
            right
                .started_at_ms
                .cmp(&left.started_at_ms)
                .then_with(|| right.run_id.cmp(&left.run_id))
        });
        Ok(runs.into_iter().next())
    }

    pub(crate) fn read_index(&self) -> Result<EvalStoreIndex> {
        let path = self.root.join("index.json");
        let index: EvalStoreIndex = serde_json::from_str(
            &fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?,
        )
        .with_context(|| format!("failed to parse {}", path.display()))?;
        reject_unsupported(index.schema_version, &path)?;
        Ok(index)
    }

    pub(crate) fn refresh_indexes(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let runs = self.scan_runs()?;
        let datasets = self.list_datasets()?;
        let index = EvalStoreIndex {
            schema_version: SCHEMA_VERSION,
            generated_at_ms: now_ms(),
            runs: runs.clone(),
            datasets,
        };
        write_json_pretty(&self.root.join("index.json"), &index)?;

        let mut by_namespace: BTreeMap<PathBuf, (String, String, Vec<RunIndexEntry>)> =
            BTreeMap::new();
        for run in runs {
            by_namespace
                .entry(run.namespace.clone())
                .or_insert_with(|| (run.project.clone(), run.project_slug.clone(), Vec::new()))
                .2
                .push(run);
        }
        for (namespace, (project, project_slug, runs)) in by_namespace {
            self.write_latest_for_namespace(&namespace, &project, &project_slug, &runs)?;
        }
        Ok(())
    }

    pub(crate) fn write_latest_for_namespace(
        &self,
        namespace: &Path,
        project: &str,
        project_slug: &str,
        runs: &[RunIndexEntry],
    ) -> Result<()> {
        let mut sorted = runs.to_vec();
        sorted.sort_by(|left, right| {
            right
                .started_at_ms
                .cmp(&left.started_at_ms)
                .then_with(|| right.run_id.cmp(&left.run_id))
        });
        let latest = sorted.first().cloned();
        let latest_index = LatestProjectIndex {
            schema_version: SCHEMA_VERSION,
            generated_at_ms: now_ms(),
            project: project.to_string(),
            project_slug: project_slug.to_string(),
            latest,
            runs: sorted,
        };
        let dir = self.root.join(namespace);
        fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
        write_json_pretty(&dir.join("latest.json"), &latest_index)
    }

    pub(crate) fn scan_runs(&self) -> Result<Vec<RunIndexEntry>> {
        let mut runs = Vec::new();
        if !self.root.is_dir() {
            return Ok(runs);
        }
        self.scan_runs_in(&self.root, &mut runs)?;
        runs.sort_by(|left, right| {
            right
                .started_at_ms
                .cmp(&left.started_at_ms)
                .then_with(|| right.run_id.cmp(&left.run_id))
        });
        Ok(runs)
    }

    pub(crate) fn scan_runs_in(&self, dir: &Path, runs: &mut Vec<RunIndexEntry>) -> Result<()> {
        if dir == self.root.join("datasets") {
            return Ok(());
        }
        if dir.join("summary.json").is_file() {
            let summary = read_run_summary(dir)?;
            runs.push(run_index_entry(&summary, dir, &self.root));
            return Ok(());
        }
        for entry in
            fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                self.scan_runs_in(&path, runs)?;
            }
        }
        Ok(())
    }

    pub(crate) fn write_dashboard(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let runs = self.list_runs()?;
        let datasets = self.list_datasets()?;
        let html = render_store_dashboard(self, &runs, &datasets);
        fs::write(self.root.join("dashboard.html"), html).with_context(|| {
            format!(
                "failed to write {}",
                self.root.join("dashboard.html").display()
            )
        })
    }
}

pub fn init_eval_store(request: InitStoreRequest) -> Result<PevalConfig> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let config_path = home.join("peval.toml");
    let root = if let Some(path) = request.root {
        resolve_explicit_path(&path, &env_map, &cwd)?
    } else {
        home_path(&env_map)?.join(".local/evals")
    };
    let config = PevalConfig {
        schema_version: SCHEMA_VERSION,
        root,
    };

    if config_path.is_file() {
        let existing = read_peval_config(&config_path)?;
        if existing.root == config.root {
            EvalStore::new(existing.root.clone()).ensure_layout()?;
            return Ok(existing);
        }
        if !request.force {
            bail!(
                "{} already points to {}; rerun `peval init --force --root {}` to replace it",
                config_path.display(),
                existing.root.display(),
                config.root.display()
            );
        }
    }

    fs::create_dir_all(&home).with_context(|| format!("failed to create {}", home.display()))?;
    write_toml_pretty(&config_path, &config)?;
    EvalStore::new(config.root.clone()).ensure_layout()?;
    Ok(config)
}

pub fn check_project(
    project: &EvalProject,
    suite_filter: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<Vec<CaseResult>> {
    let cases = expand_matrix(project, suite_filter, agent_filter)?;
    for case in &cases {
        validate_case(project, case)?;
    }
    Ok(cases
        .into_iter()
        .map(|case| CaseResult {
            schema_version: SCHEMA_VERSION,
            case_id: case.case_id,
            suite_id: case.suite.id,
            task_id: case.task.id,
            task_family: case.task.kind,
            agent_id: case.agent.id,
            status: CaseStatus::Passed,
            failure_class: None,
            score: ScoreResult {
                schema_version: SCHEMA_VERSION,
                passed: true,
                score: None,
                message: "validated".to_string(),
                details: Value::Null,
            },
            duration_ms: 0,
            artifacts: CaseArtifacts {
                result: PathBuf::new(),
                trajectory: PathBuf::new(),
                scorer_stdout: PathBuf::new(),
                scorer_stderr: PathBuf::new(),
            },
        })
        .collect())
}
